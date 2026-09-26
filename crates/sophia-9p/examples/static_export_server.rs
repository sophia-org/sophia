//! Serves the C1 static test export on a Unix socket for the independent Go
//! oracle (`tools/9p-oracle`), driven by `cargo xtask check 9p-conformance`.
//!
//! ```text
//! static_export_server --socket PATH [--mutation NAME]
//! ```
//!
//! It prints one ready line, then serves until its standard input closes.
//! A mutation deliberately breaks one property the oracle checks, so the
//! runner's self-test can show the oracle notices. Mutations live only in this
//! harness: in the export wrapper below, or in a relay that rewrites the
//! server's replies on their way to the client. The crate itself has none.

#[allow(dead_code)]
#[path = "../tests/support/export.rs"]
mod export;

use std::io::{BufRead, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use export::{Node, StaticExport};
use sophia_9p::unix::Server;
use sophia_9p::{
    Access, AttachContext, Attachment, Entry, Errno, Export, Limits, OpenFlags, ReadOutcome,
    WalkName,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mutation {
    /// The owner's check admits everything.
    PermissiveCheck,
    /// The first byte of every read is changed.
    CorruptRead,
    /// A read that would wait is answered empty at once.
    PendingAsEmpty,
    /// Rversion names the Google extension the client offered.
    VersionSuffix,
    /// Rflush never reaches the client.
    DropFlush,
}

impl Mutation {
    fn parse(name: &str) -> Result<Self, String> {
        Ok(match name {
            "permissive-check" => Self::PermissiveCheck,
            "corrupt-read" => Self::CorruptRead,
            "pending-as-empty" => Self::PendingAsEmpty,
            "version-suffix" => Self::VersionSuffix,
            "drop-flush" => Self::DropFlush,
            other => return Err(format!("unknown mutation {other:?}")),
        })
    }

    const fn in_relay(self) -> bool {
        matches!(self, Self::VersionSuffix | Self::DropFlush)
    }
}

struct Mutant {
    inner: StaticExport,
    mutation: Option<Mutation>,
}

impl Export for Mutant {
    type Node = Node;
    type Handle = export::Handle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        self.inner.attach(context)
    }

    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        let checked = self.inner.check(access);
        if self.mutation == Some(Mutation::PermissiveCheck) {
            return Ok(());
        }
        checked
    }

    fn lookup(&mut self, directory: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        self.inner.lookup(directory, name)
    }

    fn describe(&self, node: &Node, handle: Option<&Self::Handle>) -> Entry {
        self.inner.describe(node, handle)
    }

    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Self::Handle, Errno> {
        self.inner.open(node, flags)
    }

    fn read(
        &mut self,
        node: &Node,
        handle: &mut Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        match (self.inner.read(node, handle, offset, count)?, self.mutation) {
            (ReadOutcome::Ready(mut data), Some(Mutation::CorruptRead)) if !data.is_empty() => {
                data[0] ^= 0xff;
                Ok(ReadOutcome::Ready(data))
            }
            (ReadOutcome::Pending, Some(Mutation::PendingAsEmpty)) => {
                Ok(ReadOutcome::Ready(Vec::new()))
            }
            (outcome, _) => Ok(outcome),
        }
    }

    fn write(
        &mut self,
        node: &Node,
        handle: &mut Self::Handle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        self.inner.write(node, handle, offset, data)
    }

    fn release(&mut self, node: Node, handle: Option<Self::Handle>) {
        self.inner.release(node, handle);
    }
}

fn main() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (socket, mutation) = match arguments.as_slice() {
        [flag, socket] if flag == "--socket" => (PathBuf::from(socket), None),
        [flag, socket, mutation_flag, name]
            if flag == "--socket" && mutation_flag == "--mutation" =>
        {
            (PathBuf::from(socket), Some(Mutation::parse(name)?))
        }
        _ => return Err("usage: static_export_server --socket PATH [--mutation NAME]".into()),
    };
    let served = match mutation {
        Some(mutation) if mutation.in_relay() => socket.with_extension("inner"),
        _ => socket.clone(),
    };
    let export = Mutant {
        inner: StaticExport::new(),
        mutation,
    };
    let mut server = Server::new(export, Limits::default()).map_err(|error| error.to_string())?;
    let listener = UnixListener::bind(&served).map_err(|error| error.to_string())?;
    server.listen(listener).map_err(|error| error.to_string())?;
    if let Some(mutation) = mutation.filter(|mutation| mutation.in_relay()) {
        let public = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
        std::thread::spawn(move || relay(&public, &served, mutation));
    }
    let wake = server.wake();
    std::thread::spawn(move || {
        // Serve until the runner closes standard input.
        for _ in std::io::stdin().lock().lines() {}
        wake.stop();
    });
    println!(
        "sophia_9p_static_export schema=1 status=ready socket={} mutation={}",
        socket.display(),
        mutation.map_or("none".to_owned(), |mutation| format!("{mutation:?}"))
    );
    std::io::stdout()
        .flush()
        .map_err(|error| error.to_string())?;
    server.run().map_err(|error| error.to_string())
}

/// Passes requests through unchanged and rewrites replies frame by frame.
fn relay(public: &UnixListener, inner: &Path, mutation: Mutation) {
    for client in public.incoming().flatten() {
        let Ok(server) = UnixStream::connect(inner) else {
            continue;
        };
        let (Ok(mut requests_in), Ok(mut requests_out)) = (client.try_clone(), server.try_clone())
        else {
            continue;
        };
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut requests_in, &mut requests_out);
            let _ = requests_out.shutdown(Shutdown::Write);
        });
        std::thread::spawn(move || {
            let client = rewrite(server, client, mutation);
            // A server that closed closes the client's side as well.
            let _ = client.shutdown(Shutdown::Both);
        });
    }
}

fn rewrite(mut from: UnixStream, mut to: UnixStream, mutation: Mutation) -> UnixStream {
    loop {
        let mut size = [0; 4];
        if from.read_exact(&mut size).is_err() {
            return to;
        }
        let Some(rest_length) = (u32::from_le_bytes(size) as usize).checked_sub(4) else {
            return to;
        };
        let mut rest = vec![0; rest_length];
        if from.read_exact(&mut rest).is_err() {
            return to;
        }
        let kind = rest.first().copied();
        let frame = match (mutation, kind) {
            (Mutation::DropFlush, Some(109)) => continue,
            (Mutation::VersionSuffix, Some(101)) => {
                // type tag msize, then the version string replaced.
                let mut body = rest[..7].to_vec();
                let version = b"9P2000.L.Google.7";
                body.extend_from_slice(&(version.len() as u16).to_le_bytes());
                body.extend_from_slice(version);
                let mut frame = ((body.len() + 4) as u32).to_le_bytes().to_vec();
                frame.extend(body);
                frame
            }
            _ => size.iter().copied().chain(rest).collect(),
        };
        if to.write_all(&frame).is_err() {
            return to;
        }
    }
}
