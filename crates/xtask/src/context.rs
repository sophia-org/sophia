//! Bounded, source-linked notebook reads. No generated facts or persistent index.

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

const DEFAULT_LIMIT: usize = 12_000;
const USAGE: &str = "context task ID | outline PATH | read PATH[#HEADING] [--max-bytes=N]";

struct Heading {
    line: usize,
    level: usize,
    anchor: String,
}

pub fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut limit = None;
    for argument in arguments {
        if let Some(value) = argument.strip_prefix("--max-bytes=") {
            if limit.is_some() {
                return Err("duplicate --max-bytes".into());
            }
            limit = Some(value.parse::<usize>().map_err(|_| "invalid --max-bytes")?);
        } else {
            args.push(argument.as_str());
        }
    }
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let output = match args.as_slice() {
        ["read", target] => read(repo, target)?,
        ["outline", target] => outline(repo, target)?,
        ["task", id] => task(repo, id)?,
        _ => return Err(USAGE.into()),
    };
    // Refuse rather than silently drop a trailing qualification or constraint.
    if output.len() + 1 > limit {
        return Err(format!(
            "context needs {} bytes, limit is {limit}; use outline and read a smaller section, or explicitly raise --max-bytes",
            output.len() + 1
        ));
    }
    Ok(vec![output])
}

fn source(repo: &Path, name: &str) -> Result<String, String> {
    let root = repo.canonicalize().map_err(|error| error.to_string())?;
    let path = root
        .join(name)
        .canonicalize()
        .map_err(|error| format!("{name}: {error}"))?;
    if !path.starts_with(&root) || path.extension().is_none_or(|ext| ext != "md") {
        return Err("context sources must be Markdown files inside this checkout".into());
    }
    std::fs::read_to_string(path).map_err(|error| format!("{name}: {error}"))
}

fn provenance(name: &str, text: &str) -> String {
    format!(
        "source: {name}\nsha256: {:x}\n",
        Sha256::digest(text.as_bytes())
    )
}

fn headings(text: &str) -> Vec<Heading> {
    let mut result = Vec::new();
    let mut used = BTreeSet::new();
    let mut fence: Option<(char, usize)> = None;
    for (line, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        let marker = trimmed.chars().next().unwrap_or(' ');
        let count = trimmed.chars().take_while(|&c| c == marker).count();
        if let Some((active, length)) = fence {
            if marker == active && count >= length && trimmed[count..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if raw.len() - trimmed.len() > 3 {
            continue;
        }
        if matches!(marker, '`' | '~') && count >= 3 {
            fence = Some((marker, count));
            continue;
        }
        if marker != '#' || !(1..=6).contains(&count) {
            continue;
        }
        let rest = &trimmed[count..];
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let title = rest.trim().trim_end_matches('#').trim_end();
        let base: String = title
            .to_lowercase()
            .chars()
            .filter_map(|c| {
                if c.is_whitespace() {
                    Some('-')
                } else if c.is_alphanumeric() || matches!(c, '-' | '_') {
                    Some(c)
                } else {
                    None
                }
            })
            .collect();
        let mut anchor = base.clone();
        let mut suffix = 0;
        while !used.insert(anchor.clone()) {
            suffix += 1;
            anchor = format!("{base}-{suffix}");
        }
        result.push(Heading {
            line,
            level: count,
            anchor,
        });
    }
    result
}

fn outline(repo: &Path, name: &str) -> Result<String, String> {
    let text = source(repo, name)?;
    let mut output = provenance(name, &text);
    for heading in headings(&text) {
        output.push_str(&format!(
            "L{} {} #{anchor}\n",
            heading.line + 1,
            "#".repeat(heading.level),
            anchor = heading.anchor
        ));
    }
    Ok(output)
}

fn read(repo: &Path, target: &str) -> Result<String, String> {
    let (name, anchor) = target
        .split_once('#')
        .map_or((target, None), |(p, a)| (p, Some(a)));
    let text = source(repo, name)?;
    let lines: Vec<_> = text.lines().collect();
    let headings = headings(&text);
    let (start, end) = if let Some(anchor) = anchor {
        let index = headings
            .iter()
            .position(|h| h.anchor == anchor)
            .ok_or_else(|| format!("heading #{anchor} not found in {name}; use context outline"))?;
        let selected = &headings[index];
        let end = headings[index + 1..]
            .iter()
            .find(|h| h.level <= selected.level)
            .map_or(lines.len(), |h| h.line);
        (selected.line, end)
    } else {
        // A bare path reads its introductory text, never the entire history.
        (
            0,
            headings
                .iter()
                .find(|h| h.level >= 2)
                .map_or(lines.len(), |h| h.line),
        )
    };
    Ok(format!(
        "{}lines: {}-{} of {}\n\n{}",
        provenance(target, &text),
        start + 1,
        end,
        lines.len(),
        lines[start..end].join("\n")
    ))
}

fn task(repo: &Path, id: &str) -> Result<String, String> {
    if !id
        .strip_prefix('t')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err("task ID must be t followed by digits".into());
    }
    let text = source(repo, "todo.md")?;
    let key = format!("id:{id}");
    let rows: Vec<_> = text
        .lines()
        .filter(|line| !line.starts_with('#') && line.split_whitespace().any(|part| part == key))
        .collect();
    let [row] = rows.as_slice() else {
        return Err(format!("expected one open {id} row, found {}", rows.len()));
    };
    let target = row
        .split_once("[details](")
        .and_then(|(_, tail)| tail.split_once(')').map(|(target, _)| target))
        .ok_or_else(|| format!("{id} has no [details] link"))?;
    Ok(format!(
        "{}\n{row}\n\n{}",
        provenance("todo.md", &text),
        read(repo, target)?
    ))
}
