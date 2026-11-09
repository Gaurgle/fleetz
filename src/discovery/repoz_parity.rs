use super::*;
use std::process::Command;

fn repoz_bin() -> Option<PathBuf> {
    // The clone next to this one first, so the test checks the repoz you
    // are actually developing rather than whatever is installed.
    let sibling = dirs::home_dir()?.join("repos/repoz/repoz");
    if sibling.is_file() {
        return Some(sibling);
    }
    let out = Command::new("sh")
        .args(["-c", "command -v repoz"])
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn sh(dir: &Path, args: &[&str]) {
    let status = Command::new(args[0])
        .args(&args[1..])
        .current_dir(dir)
        // As in `repoz_paths`: inherited from a git hook, these would
        // aim the fixture's git calls at the checkout running the tests.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .status()
        .expect("spawn");
    assert!(status.success(), "command failed: {args:?}");
}

/// A git repo at `base/rel` with one commit and one uncommitted change.
fn dirty_repo(base: &Path, rel: &str) -> PathBuf {
    let dir = base.join(rel);
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, &["git", "init", "-q", "-b", "main"]);
    sh(&dir, &["git", "config", "user.email", "t@example.com"]);
    sh(&dir, &["git", "config", "user.name", "t"]);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    sh(&dir, &["git", "add", "."]);
    sh(&dir, &["git", "commit", "-q", "-m", "init"]);
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    dir
}

/// Absolute repo paths repoz reports in dirty mode. Rows are
/// `<name> <path> ··· <status>`, colored, so the escapes come off first
/// and the path is the field that starts with a slash.
///
/// Run from the parent of `root`, outside any repository and outside the
/// scan root, because repoz adds the git toplevel of its working
/// directory to the scan. Inherited from `cargo test`, that was the fleetz
/// checkout running the tests, and a dirty one showed up next to the
/// fixture; under `$HOME` it was hidden only by luck, since repoz prints
/// such a path as `~/...` and the slash filter below drops it. Run from
/// `root` itself, the hidden-root test's `root` is a repo, and its
/// toplevel would give repoz a second route to the fixture, hiding a
/// repoz that stopped scanning a hidden `REPO_CHECK_DIR`. The parent is
/// the system temp directory or the plain fixture base, so
/// `REPO_CHECK_DIR` is repoz's only way in. `GIT_DIR` and `GIT_WORK_TREE`
/// go too: a git hook running `cargo test` would redirect repoz's git.
fn repoz_paths(bin: &Path, root: &Path) -> Vec<PathBuf> {
    let out = Command::new(bin)
        .arg("-d")
        .current_dir(root.parent().unwrap_or(root))
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("REPO_CHECK_DIR", root)
        // A real config would add the user's own EXTRA_DIRS to the scan.
        .env("XDG_CONFIG_HOME", root.join("no-config"))
        .output()
        .expect("run repoz");
    let stdout = String::from_utf8_lossy(&out.stdout);

    let mut paths: Vec<PathBuf> = Vec::new();
    for line in strip_ansi(&stdout).lines() {
        for word in line.split_whitespace() {
            if word.starts_with('/') && Path::new(word).join(".git").exists() {
                paths.push(PathBuf::from(word));
            }
        }
    }
    paths.sort();
    paths
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // Consume through the final byte of the CSI sequence.
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                break;
            }
        }
    }
    out
}

fn fixture(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("fleetz_parity_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    // Resolved, because on macOS `temp_dir()` hands back `/var/...` while
    // `/var` is a symlink to `/private/var`. repoz's `find` reports the
    // resolved path and fleetz reports the one it was given, so the two
    // agreed about every repo and still failed on the spelling of the
    // root. Neither tool is wrong; the fixture was handing them different
    // questions.
    std::fs::canonicalize(&base).unwrap_or(base)
}

#[test]
fn discovers_the_same_repos_as_repoz() {
    let Some(bin) = repoz_bin() else {
        eprintln!("SKIP: repoz not found, parity unchecked");
        return;
    };
    let base = fixture("same");

    // The shapes that have actually diverged or plausibly could: a plain
    // repo, a nested one, a dot-prefixed clone (the `~/.dotfiles` case),
    // and a repo inside an otherwise ordinary hidden directory.
    let mut expected = vec![
        dirty_repo(&base, "alpha"),
        dirty_repo(&base, "nested/beta"),
        dirty_repo(&base, ".dotfiles"),
        dirty_repo(&base, ".config/gamma"),
    ];
    // A pruned directory must be invisible to both.
    dirty_repo(&base, "node_modules/should-not-appear");
    expected.sort();

    let theirs = repoz_paths(&bin, &base);
    let mut ours = find_all_local(std::slice::from_ref(&base));
    ours.sort();

    assert_eq!(
        ours, theirs,
        "\nfleetz found: {ours:#?}\nrepoz found:  {theirs:#?}"
    );
    assert_eq!(ours, expected, "neither tool found the expected set");

    std::fs::remove_dir_all(base).ok();
}

#[test]
fn agrees_with_repoz_on_a_hidden_scan_root() {
    // `repo_dir` or an `extra_dir` pointed straight at a hidden directory:
    // the shape that made `~/.dotfiles` invisible in fleetz.
    let Some(bin) = repoz_bin() else {
        eprintln!("SKIP: repoz not found, parity unchecked");
        return;
    };
    let base = fixture("hidden_root");
    let root = base.join(".dotfiles");
    let expected = vec![dirty_repo(&base, ".dotfiles")];

    let theirs = repoz_paths(&bin, &root);
    let ours = find_all_local(std::slice::from_ref(&root));

    assert_eq!(ours, theirs, "\nfleetz: {ours:#?}\nrepoz:  {theirs:#?}");
    assert_eq!(ours, expected);

    std::fs::remove_dir_all(base).ok();
}
