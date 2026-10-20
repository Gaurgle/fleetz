//! Write a commit hash to the system clipboard (FZ-37).
//!
//! The first clipboard command found on `PATH` gets the text on its stdin,
//! bounded in time; with none installed the text goes to the terminal as an
//! OSC 52 sequence, which reaches the local clipboard through ssh and tmux
//! wherever the terminal allows it. Write only: nothing here reads the
//! clipboard back.

use std::ffi::OsStr;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Clipboard commands in the order they are tried. The first word is looked
/// up on `PATH`; the rest are its arguments.
pub const COMMANDS: &[&[&str]] = &[
    &["pbcopy"],
    &["wl-copy"],
    &["xclip", "-selection", "clipboard"],
    &["xsel", "--clipboard", "--input"],
];

/// How long a clipboard command may run before it is killed and the copy
/// reported as failed.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// Copy `text` with the machine's own clipboard, or OSC 52 on the terminal.
pub fn copy(text: &str) -> Result<(), String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    copy_with(text, COMMANDS, &path, TIMEOUT, &mut std::io::stdout())
}

/// `copy` with every input the machine would otherwise supply passed in, so a
/// test can point it at a fake command on a temporary `PATH`.
pub fn copy_with(
    text: &str,
    commands: &[&[&str]],
    path: &OsStr,
    timeout: Duration,
    terminal: &mut dyn Write,
) -> Result<(), String> {
    match commands
        .iter()
        .find_map(|cmd| Some((find_on_path(cmd.first()?, path)?, *cmd)))
    {
        Some((program, cmd)) => run(&program, cmd, text, timeout),
        None => terminal
            .write_all(osc52(text).as_bytes())
            .and_then(|()| terminal.flush())
            .map_err(|e| format!("terminal: {e}")),
    }
}

/// The OSC 52 sequence that sets the clipboard to `text`.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// Standard base64 with padding. A dozen lines rather than a dependency.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            match i <= chunk.len() {
                true => out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char),
                false => out.push('='),
            }
        }
    }
    out
}

/// The executable `name` resolves to on `path`, if any.
fn find_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}

/// Run one clipboard command with `text` on its stdin, killed past `timeout`.
fn run(program: &PathBuf, cmd: &[&str], text: &str, timeout: Duration) -> Result<(), String> {
    let name = cmd[0];
    let mut child = Command::new(program)
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{name}: {e}"))?;
    // Dropped at the end of the statement, closing stdin so the command sees
    // the end of its input.
    let written = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(text.as_bytes()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name} timed out"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(format!("{name}: {e}")),
        }
    };
    if let Some(Err(e)) = written {
        return Err(format!("{name}: {e}"));
    }
    match status.success() {
        true => Ok(()),
        false => Err(format!("{name} exited with {status}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PATH` holding only `dir`.
    fn path_of(dir: &std::path::Path) -> std::ffi::OsString {
        std::env::join_paths([dir]).expect("joinable")
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fleetz-clipboard-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    fn fake_command(dir: &std::path::Path, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want, "{input:?}");
        }
    }

    #[test]
    fn osc52_for_a_known_hash() {
        assert_eq!(osc52("1c51c6e"), "\x1b]52;c;MWM1MWM2ZQ==\x07");
    }

    #[cfg(unix)]
    #[test]
    fn the_first_command_on_path_receives_the_hash() {
        let dir = scratch("first");
        let got = dir.join("got");
        // Only the second command in the list exists, so the lookup has to
        // walk past a missing one, and its arguments have to arrive.
        fake_command(
            &dir,
            "xclip",
            &format!("echo \"$@\" > '{0}.args'; cat > '{0}'", got.display()),
        );
        let mut terminal = Vec::new();
        let commands: &[&[&str]] = &[&["pbcopy"], &["xclip", "-selection", "clipboard"]];
        copy_with("1c51c6e", commands, &path_of(&dir), TIMEOUT, &mut terminal).expect("copied");
        assert_eq!(std::fs::read_to_string(&got).unwrap(), "1c51c6e");
        assert_eq!(
            std::fs::read_to_string(dir.join("got.args")).unwrap(),
            "-selection clipboard\n"
        );
        assert!(terminal.is_empty(), "no OSC 52 when a command ran");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_command_is_reported() {
        let dir = scratch("fails");
        fake_command(&dir, "pbcopy", "cat > /dev/null; exit 3");
        let err = copy_with(
            "1c51c6e",
            COMMANDS,
            &path_of(&dir),
            TIMEOUT,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(err.starts_with("pbcopy exited with"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_hung_command_is_killed_at_the_bound() {
        let dir = scratch("hangs");
        fake_command(&dir, "wl-copy", "exec sleep 5");
        let started = Instant::now();
        let err = copy_with(
            "1c51c6e",
            COMMANDS,
            &path_of(&dir),
            Duration::from_millis(200),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(err, "wl-copy timed out");
        assert!(started.elapsed() < Duration::from_secs(3), "not bounded");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_no_command_the_terminal_gets_osc52() {
        let dir = scratch("none");
        let mut terminal = Vec::new();
        copy_with("1c51c6e", COMMANDS, &path_of(&dir), TIMEOUT, &mut terminal).expect("copied");
        assert_eq!(terminal, b"\x1b]52;c;MWM1MWM2ZQ==\x07");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that is not executable is not a command, so a stray `pbcopy`
    /// text file cannot swallow the copy.
    #[cfg(unix)]
    #[test]
    fn a_non_executable_file_is_skipped() {
        let dir = scratch("noexec");
        std::fs::write(dir.join("pbcopy"), "not a program").unwrap();
        let mut terminal = Vec::new();
        copy_with("abc1234", COMMANDS, &path_of(&dir), TIMEOUT, &mut terminal).expect("copied");
        assert_eq!(terminal, osc52("abc1234").as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
