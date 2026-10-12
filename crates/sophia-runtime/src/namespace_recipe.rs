//! Pure, bounded compilation of trusted naming instructions. No filesystem,
//! socket, process or grant is opened or allocated here.
use sophia_protocol::{NamespaceId, namespace_view::*};
use std::collections::{BTreeMap, BTreeSet};

mod tree;
use tree::{Node, normalize};

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_OPERATIONS: usize = 1024;
pub const MAX_INCLUDE_DEPTH: usize = 8;
pub const MAX_INCLUDE_FILES: usize = 32;
/// Empty-directory aliases also consume space. Bound those independently of
/// endpoints so repeated snapshots cannot amplify an empty tree without limit.
pub const MAX_TREE_NODES: usize = 65536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Syntax,
    Identity,
    Scope,
    Path,
    Missing,
    Conflict,
    Kind,
    IncludeCycle,
    Limit,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "namespace recipe refused: {self:?}")
    }
}
impl std::error::Error for Error {}

/// Files are an immutable, owner-pinned snapshot under a trusted config root.
/// The loader rejects symlinks/changes before supplying this map. Relative
/// include paths resolve from that root, never from compiler `cd`.
pub struct Input<'a> {
    pub revision: u64,
    pub namespace: NamespaceId,
    pub principal: u64,
    pub source: &'a str,
    pub fragments: &'a BTreeMap<String, String>,
    pub services: &'a BTreeMap<String, GrantedEndpoint>,
}

pub fn compile(input: Input<'_>) -> Result<CompiledNamespace, Error> {
    if input.revision == 0 || !input.namespace.is_valid() || input.principal == 0 {
        return Err(Error::Identity);
    }
    let mut compiler = Compiler {
        input,
        root: Node::Directory(Vec::new()),
        cwd: Vec::new(),
        bytes: 0,
        operations: 0,
        active: BTreeSet::new(),
        included: BTreeSet::new(),
    };
    compiler.expand(compiler.input.source, 0, true)?;
    let mut endpoints = Vec::new();
    compiler.root.flatten("", &mut endpoints);
    Ok(CompiledNamespace {
        revision: compiler.input.revision,
        namespace: compiler.input.namespace,
        principal: compiler.input.principal,
        endpoints,
    })
}

struct Compiler<'a> {
    input: Input<'a>,
    root: Node,
    cwd: Vec<String>,
    bytes: usize,
    operations: usize,
    active: BTreeSet<String>,
    included: BTreeSet<String>,
}

impl Compiler<'_> {
    fn expand(&mut self, source: &str, depth: usize, top: bool) -> Result<(), Error> {
        self.bytes = self.bytes.checked_add(source.len()).ok_or(Error::Limit)?;
        if self.bytes > MAX_SOURCE_BYTES || depth > MAX_INCLUDE_DEPTH {
            return Err(Error::Limit);
        }
        let mut header = !top;
        for line in source.split('\n') {
            // LF is the only line separator. Other control bytes, including
            // CR and NUL, cannot disguise tokens or discovery fields.
            if line.chars().any(|c| c.is_control() && c != '\t') {
                return Err(Error::Syntax);
            }
            let line = line.trim_matches([' ', '\t']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let words: Vec<_> = line.split([' ', '\t']).filter(|s| !s.is_empty()).collect();
            if !header {
                if words != ["namespace-recipe", "1"] {
                    return Err(Error::Syntax);
                }
                header = true;
                continue;
            }
            self.operations += 1;
            if self.operations > MAX_OPERATIONS {
                return Err(Error::Limit);
            }
            match words.as_slice() {
                ["mount", service, target] => {
                    let endpoint = self.input.services.get(*service).ok_or(Error::Missing)?;
                    self.check_scope(endpoint)?;
                    let target = normalize(&self.cwd, target)?;
                    self.root
                        .install(&target, Node::Endpoint(endpoint.clone()), None)?;
                }
                ["bind", source, target] => self.bind(source, target, None)?,
                ["bind", flag @ ("-a" | "-b"), source, target] => {
                    self.bind(source, target, Some(*flag == "-b"))?;
                }
                ["unmount", target] => {
                    let target = normalize(&self.cwd, target)?;
                    self.root.remove(&target)?;
                }
                ["cd", target] => self.cwd = normalize(&self.cwd, target)?,
                ["include", file] => {
                    if file.starts_with('/') {
                        return Err(Error::Path);
                    }
                    let path = normalize(&[], file)?.join("/");
                    if path.is_empty() {
                        return Err(Error::Path);
                    }
                    if !self.active.insert(path.clone()) {
                        return Err(Error::IncludeCycle);
                    }
                    self.included.insert(path.clone());
                    if self.included.len() > MAX_INCLUDE_FILES {
                        return Err(Error::Limit);
                    }
                    let fragment = self.input.fragments.get(&path).ok_or(Error::Missing)?;
                    self.expand(fragment, depth + 1, false)?;
                    self.active.remove(&path);
                }
                _ => return Err(Error::Syntax),
            }
            if self.root.endpoint_count() > MAX_ENDPOINTS {
                return Err(Error::Limit);
            }
            self.root.validate(0, &mut 0)?;
        }
        if !header {
            return Err(Error::Syntax);
        }
        Ok(())
    }

    fn check_scope(&self, endpoint: &GrantedEndpoint) -> Result<(), Error> {
        let scope = endpoint.scope;
        if scope.namespace != self.input.namespace || scope.principal != self.input.principal {
            return Err(Error::Scope);
        }
        let service = &endpoint.service;
        if scope.grant == 0
            || scope.generation == 0
            || service.identity == 0
            || service.generation == 0
            || service.version == 0
            || service.interface.is_empty()
            || service.interface.len() > MAX_COMPONENT_BYTES
            || !service
                .interface
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Error::Identity);
        }
        Ok(())
    }

    fn bind(&mut self, source: &str, target: &str, prepend: Option<bool>) -> Result<(), Error> {
        let source = normalize(&self.cwd, source)?;
        let target = normalize(&self.cwd, target)?;
        // Capture before installation, including when source and target
        // overlap: subsequent edits cannot retarget aliases or recurse.
        let captured = self.root.lookup(&source)?.clone();
        self.root.install(&target, captured, prepend)
    }
}
