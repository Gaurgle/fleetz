//! Print mode: the one-shot text report bash repoz prints, rebuilt on fleetz's
//! own discovery, git and GitHub code (`docs/repoz-print-design.md`).
//!
//! Only the golden harness exists so far (FZ-46, `parity`). The report itself
//! lands in FZ-47; until then `run` is a placeholder that writes nothing.

use std::io::{self, Write};

/// Write the report for the repoz-style arguments `args` (`-a`, `--since
/// 2026-03-01`, ...) to `out`. A placeholder until FZ-47: it writes nothing,
/// and nothing outside the harness calls it yet.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "print mode is wired into main in FZ-47")
)]
pub fn run(args: &[String], out: &mut dyn Write) -> io::Result<()> {
    let _ = (args, out);
    Ok(())
}

#[cfg(test)]
pub mod parity;
