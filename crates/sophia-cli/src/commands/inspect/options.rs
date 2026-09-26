use std::path::PathBuf;

pub(super) enum Operation {
    Ls,
    Stat(String),
    Status,
    Snapshot,
    Watch,
}
pub(super) struct Options {
    pub socket: PathBuf,
    pub json: bool,
    pub operation: Operation,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Self, &'static str> {
        let Some((domain, args)) = args.split_first() else {
            return Err("expected inspect wm");
        };
        if domain != "wm" {
            return Err("expected inspect wm");
        }
        let mut socket = None;
        let mut json = false;
        let mut operands = Vec::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--socket" => {
                    if socket.is_some() {
                        return Err("duplicate --socket");
                    }
                    socket = Some(PathBuf::from(
                        args.next().ok_or("--socket needs an absolute path")?,
                    ));
                }
                "--json" => {
                    if json {
                        return Err("duplicate --json");
                    }
                    json = true;
                }
                value if value.starts_with('-') => return Err("unknown inspect option"),
                _ => operands.push(arg.as_str()),
            }
        }
        let operation = match operands.as_slice() {
            ["ls"] => Operation::Ls,
            ["status"] => Operation::Status,
            ["snapshot"] => Operation::Snapshot,
            ["watch"] => Operation::Watch,
            ["stat", name] => {
                let name = if *name == "/" {
                    "/"
                } else {
                    name.strip_prefix('/').unwrap_or(name)
                };
                if name != "/" && !super::NAMES.contains(&name) {
                    return Err("unknown inspection path");
                }
                Operation::Stat(name.to_owned())
            }
            _ => return Err("expected ls, stat PATH, status, snapshot or watch"),
        };
        let socket = socket
            .or_else(|| {
                std::env::var_os(sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV).map(PathBuf::from)
            })
            .ok_or(
                "pass --socket or set SOPHIA_WM_INSPECT_SOCKET; inspection is disabled by default",
            )?;
        if !socket.is_absolute() {
            return Err("inspection socket path must be absolute");
        }
        Ok(Self {
            socket,
            json,
            operation,
        })
    }
}
