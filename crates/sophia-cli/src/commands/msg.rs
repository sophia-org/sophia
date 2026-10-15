use sophia_protocol::{ControlCommand, ControlOwner};
use sophia_runtime::{AdminClient, ControlClient, SOPHIA_CONTROL_SOCKET_ENV};

enum MsgClient {
    Legacy(ControlClient),
    Files(Box<AdminClient>),
}

impl MsgClient {
    fn commands(&mut self) -> std::io::Result<sophia_protocol::ControlCatalog> {
        match self {
            Self::Legacy(c) => c.commands(),
            Self::Files(c) => c.commands(),
        }
    }
    fn invoke(
        &mut self,
        command: ControlCommand,
    ) -> std::io::Result<(u64, sophia_protocol::ControlOutcome, String)> {
        match self {
            Self::Legacy(c) => c.invoke(command),
            Self::Files(c) => c.invoke(command),
        }
    }
    fn invocation_pending(&self) -> bool {
        match self {
            Self::Legacy(c) => c.invocation_pending(),
            Self::Files(c) => c.invocation_pending(),
        }
    }
    fn request_id(&self) -> u64 {
        match self {
            Self::Legacy(_) => 2,
            Self::Files(c) => c.request_id(),
        }
    }
}

// This tiny JSON string encoder avoids coupling the wire to a serialization
// framework. The CLI is the only JSON boundary; names remain exact strings.
fn quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch if ch <= '\u{1f}' => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub(super) fn run(args: &[String]) -> i32 {
    let mut path = std::env::var_os(SOPHIA_CONTROL_SOCKET_ENV).map(std::path::PathBuf::from);
    let mut json = false;
    let mut files = true;
    let mut positionals = Vec::new();
    let mut options = true;
    let mut index = 0;
    while index < args.len() {
        if !options {
            positionals.push(args[index].as_str());
            index += 1;
            continue;
        }
        match args[index].as_str() {
            "--" => options = false,
            "--json" => json = true,
            "--transport" => {
                index += 1;
                files = match args.get(index).map(String::as_str) {
                    Some("9p2000.L") => true,
                    Some("current-ipc") => false,
                    _ => {
                        return failure("--transport expects 9p2000.L or current-ipc", false, json);
                    }
                };
            }
            "--socket" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return failure("--socket needs a path", false, json);
                };
                path = Some(value.into());
            }
            "--help" | "-h" => {
                println!(
                    "sophia msg [--transport current-ipc|9p2000.L] [--socket PATH] [--json] commands\nsophia msg [--transport current-ipc|9p2000.L] [--socket PATH] [--json] policy <registered-name>\nsophia msg [--transport current-ipc|9p2000.L] [--socket PATH] [--json] session restart-wm|reload-profile|logout"
                );
                return 0;
            }
            arg if arg.starts_with('-') => return failure("unknown msg option", false, json),
            _ => positionals.push(args[index].as_str()),
        }
        index += 1;
    }
    let command = match positionals.as_slice() {
        ["commands"] => None,
        [owner @ ("policy" | "session"), name] => Some(ControlCommand {
            owner: if *owner == "policy" {
                ControlOwner::Policy
            } else {
                ControlOwner::Session
            },
            name: (*name).to_owned(),
        }),
        _ => {
            return failure(
                "expected commands, policy <registered-name>, or session restart-wm|reload-profile|logout",
                false,
                json,
            );
        }
    };
    let Some(path) = path else {
        return failure(
            "set SOPHIA_CONTROL_SOCKET or pass --socket; control is opt-in",
            false,
            json,
        );
    };
    let connected = if files {
        AdminClient::connect(&path)
            .map(Box::new)
            .map(MsgClient::Files)
    } else {
        ControlClient::connect(&path).map(MsgClient::Legacy)
    };
    let mut client = match connected {
        Ok(client) => client,
        Err(error) => return failure(&error.to_string(), false, json),
    };
    let catalog = match client.commands() {
        Ok(catalog) => catalog,
        Err(error) => return failure(&error.to_string(), false, json),
    };
    if let Some(command) = command {
        match client.invoke(command) {
            Ok((generation, outcome, detail)) => {
                if json {
                    println!(
                        "{{\"kind\":133,\"request_id\":{},\"catalog_generation\":{generation},\"outcome\":{},\"detail\":{}}}",
                        client.request_id(),
                        quoted(outcome.name()),
                        quoted(&detail)
                    );
                } else if detail.is_empty() {
                    println!("{}", outcome.name());
                } else {
                    println!("{}: {detail}", outcome.name());
                }
                i32::from(!outcome.success())
            }
            Err(error) => failure(&error.to_string(), client.invocation_pending(), json),
        }
    } else {
        if json {
            let entries = catalog
                .commands
                .iter()
                .map(|command| {
                    format!(
                        "{{\"owner\":{},\"completion\":{},\"name\":{}}}",
                        quoted(command.owner.name()),
                        quoted(if command.owner == ControlOwner::Policy {
                            "policy-commit"
                        } else {
                            "session-settlement"
                        }),
                        quoted(&command.name)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "{{\"kind\":131,\"request_id\":1,\"catalog_generation\":{},\"commands\":[{entries}]}}",
                catalog.generation
            );
        } else {
            for command in catalog.commands {
                println!("{} {}", command.owner.name(), command.name);
            }
        }
        0
    }
}

fn failure(detail: &str, uncertain: bool, json: bool) -> i32 {
    let outcome = if uncertain { "unknown" } else { "not-invoked" };
    if json {
        eprintln!(
            "{{\"error\":{},\"outcome\":{}}}",
            quoted(detail),
            quoted(outcome)
        );
    } else {
        eprintln!("sophia msg: {detail} (outcome: {outcome})");
    }
    2
}
