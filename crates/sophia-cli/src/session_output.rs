use std::io::{self, Write};

pub(super) fn write_line(output: &mut impl Write, line: &str) -> io::Result<()> {
    // Keep the newline in the same write even when the line exceeds stdout's buffer.
    output.write_all(format!("{line}\n").as_bytes())
}

#[cfg(test)]
#[path = "../tests/support/session_output.rs"]
mod tests;
