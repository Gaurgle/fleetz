use super::*;
use std::os::unix::process::ExitStatusExt;

fn exited(code: i32, stderr: &str) -> FetchRun {
    FetchRun::Exited(std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    })
}

fn kind_of(stderr: &str) -> FetchFailureKind {
    match fetch_outcome(exited(128, stderr)) {
        FetchOutcome::Failed(f) => f.kind,
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_zero_exit_is_a_success() {
    assert_eq!(fetch_outcome(exited(0, "")), FetchOutcome::Succeeded);
}

/// `run_git` returns `None` for both; a fetch keeps them apart.
#[test]
fn a_timeout_and_a_failure_to_start_are_told_apart() {
    let FetchOutcome::Failed(timed_out) = fetch_outcome(FetchRun::TimedOut) else {
        panic!("a timeout is a failure");
    };
    let FetchOutcome::Failed(not_started) = fetch_outcome(FetchRun::NotStarted) else {
        panic!("a failure to start is a failure");
    };
    assert_eq!(timed_out.kind, FetchFailureKind::TimedOut);
    assert_eq!(not_started.kind, FetchFailureKind::Other);
    assert_ne!(timed_out, not_started);
}

/// Messages as git 2.39, curl and OpenSSH print them under `LC_ALL=C`.
#[test]
fn git_error_messages_map_to_their_kind() {
    use FetchFailureKind::*;
    for (stderr, want) in [
        (
            "fatal: Authentication failed for 'https://github.com/o/r.git/'",
            Auth,
        ),
        (
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            Auth,
        ),
        (
            "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
            Ssh,
        ),
        (
            "Host key verification failed.\nfatal: Could not read from remote repository.",
            Auth,
        ),
        (
            "fatal: unable to access 'https://nohost.invalid/o/r.git/': Could not resolve host: nohost.invalid",
            Unreachable,
        ),
        (
            "ssh: Could not resolve hostname nohost: nodename nor servname provided, or not known",
            Unreachable,
        ),
        (
            "ssh: connect to host h port 22: Connection refused",
            Unreachable,
        ),
        (
            "fatal: '/x' does not appear to be a git repository\nfatal: Could not read from remote repository.",
            NoRemote,
        ),
        (
            "ERROR: Repository not found.\nfatal: Could not read from remote repository.",
            NoRemote,
        ),
        // macOS OpenSSH with the network down, git 2.39.3, as pasted.
        (
            "ssh: connect to host github.com port 22: Undefined error: 0\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n",
            Unreachable,
        ),
        (
            "ssh: Could not resolve hostname fleetz-nonexistent.invalid: nodename nor servname provided, or not known\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n",
            Unreachable,
        ),
        (
            "ssh: connect to host github.com port 22: Network is unreachable\nfatal: Could not read from remote repository.",
            Unreachable,
        ),
        (
            "ssh: connect to host github.com port 22: Operation timed out\nfatal: Could not read from remote repository.",
            Unreachable,
        ),
        // The generic fatal lines alone say nothing about why.
        (
            "fatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n",
            Other,
        ),
        // A refused key is Ssh even if a connect line came first.
        (
            "ssh: connect to host h port 22: Undefined error: 0\ngit@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
            Ssh,
        ),
        // git 2.39.3 on an HTTP 404 (a removed or renamed remote).
        (
            "fatal: repository 'http://127.0.0.1:18765/nope.git/' not found",
            NoRemote,
        ),
        (
            "fatal: refusing to fetch into branch 'refs/heads/main'",
            Other,
        ),
        ("", Other),
    ] {
        assert_eq!(kind_of(stderr), want, "{stderr:?}");
    }
}

/// FZ-72 (#128): ssh refusing the key is `Ssh`, whatever methods it
/// lists and whatever it printed first; an https credential refusal stays
/// `Auth`. Wording as OpenSSH, git and curl print it under `LC_ALL=C`.
#[test]
fn an_ssh_key_refusal_is_ssh_and_an_https_refusal_is_auth() {
    use FetchFailureKind::*;
    for (stderr, want) in [
        (
            "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
            Ssh,
        ),
        (
            "git@host: Permission denied (publickey,password).\nfatal: Could not read from remote repository.",
            Ssh,
        ),
        (
            "sign_and_send_pubkey: signing failed for ED25519 \"/k\" from agent: agent refused operation\ngit@github.com: Permission denied (publickey).",
            Ssh,
        ),
        ("no such identity: /k: No such file or directory", Ssh),
        (
            "ssh: connect to host h port 22: Undefined error: 0\ngit@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
            Ssh,
        ),
        (
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            Auth,
        ),
        (
            "fatal: Authentication failed for 'https://github.com/o/r.git/'",
            Auth,
        ),
        (
            "fatal: unable to access 'https://h/o/r.git/': The requested URL returned error: 401",
            Auth,
        ),
        (
            "fatal: unable to access 'https://h/o/r.git/': The requested URL returned error: 403",
            Auth,
        ),
        // A local file git cannot open is no ssh refusal.
        (
            "error: cannot open .git/FETCH_HEAD: Permission denied",
            Other,
        ),
    ] {
        assert_eq!(kind_of(stderr), want, "{stderr:?}");
    }
}

#[test]
fn the_detail_is_the_first_error_line_only() {
    let FetchOutcome::Failed(failure) = fetch_outcome(exited(
        128,
        "warning: something first\nfatal: '/x' does not appear to be a git repository\nfatal: Could not read from remote repository.\n",
    )) else {
        panic!("a failure");
    };
    assert_eq!(
        failure.detail.as_deref(),
        Some("fatal: '/x' does not appear to be a git repository")
    );
}

/// Rule 8: whatever git prints, a credential in a URL never reaches the
/// detail, and nothing in it can drive the terminal.
#[test]
fn the_detail_carries_no_userinfo_or_control_characters() {
    let FetchOutcome::Failed(failure) = fetch_outcome(exited(
        128,
        "fatal: unable to access 'https://user:secret@host.invalid/o/r.git/': \u{1b}[31mCould not resolve host",
    )) else {
        panic!("a failure");
    };
    let detail = failure.detail.unwrap();
    assert!(
        !detail.contains("secret") && !detail.contains("user"),
        "{detail}"
    );
    assert!(!detail.contains('\u{1b}'), "{detail:?}");
    assert!(detail.contains("https://host.invalid/o/r.git/"), "{detail}");
}

/// The owner's case: with the network down the fatal line is generic and
/// the ssh line before it is the one that says why.
#[test]
fn the_detail_prefers_the_ssh_line_that_says_why() {
    let FetchOutcome::Failed(failure) = fetch_outcome(exited(
        128,
        "ssh: connect to host github.com port 22: Undefined error: 0\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n",
    )) else {
        panic!("a failure");
    };
    assert_eq!(failure.kind, FetchFailureKind::Unreachable);
    assert_eq!(
        failure.detail.as_deref(),
        Some("ssh: connect to host github.com port 22: Undefined error: 0")
    );
}

/// The detail is built inside the spawned fetch task: a non-ASCII
/// character before `scheme://` in git's line must not panic there.
#[test]
fn a_detail_with_a_multibyte_character_before_the_url_does_not_panic() {
    let FetchOutcome::Failed(failure) = fetch_outcome(exited(
        128,
        "fatal: unable to access \u{2018}https://u:secret@host.invalid/o/r.git/\u{2019}: Could not resolve host",
    )) else {
        panic!("a failure");
    };
    let detail = failure.detail.unwrap();
    assert!(!detail.contains("secret"), "{detail}");
    assert!(
        detail.contains("\u{2018}https://host.invalid/o/r.git/"),
        "{detail}"
    );
}

#[test]
fn a_long_error_line_is_capped() {
    let FetchOutcome::Failed(failure) =
        fetch_outcome(exited(1, &format!("fatal: {}", "x".repeat(500))))
    else {
        panic!("a failure");
    };
    assert_eq!(failure.detail.unwrap().chars().count(), FETCH_DETAIL_MAX);
}

#[test]
fn userinfo_is_removed_from_every_url_form_that_can_carry_it() {
    for (url, want) in [
        (
            "https://user:secret@github.com/Gaurgle/fleetz.git",
            "https://github.com/Gaurgle/fleetz.git",
        ),
        (
            "https://ghp_token@github.com/o/r.git",
            "https://github.com/o/r.git",
        ),
        // An ssh user is a login name, not a credential: only a
        // password goes.
        (
            "ssh://git@github.com:22/o/r.git",
            "ssh://git@github.com:22/o/r.git",
        ),
        ("ssh://u:pw@host/x", "ssh://u@host/x"),
        ("HTTPS://tok@host/x", "HTTPS://host/x"),
        ("git+ssh://u:pw@host/x", "git+ssh://u@host/x"),
        // Every scheme outside the ssh family loses all userinfo.
        ("ftp://tok@host/x", "ftp://host/x"),
        ("persistent-https://tok@host/x", "persistent-https://host/x"),
        // `,` `)` and `'` are legal in userinfo: they must not end it.
        ("https://user:abc,def@host/r.git", "https://host/r.git"),
        ("https://user:p)ss@host/r.git", "https://host/r.git"),
        ("https://user:it's@host/r.git", "https://host/r.git"),
        ("https://u:a:b:c@host/x", "https://host/x"),
        ("https://u:p@[::1]:8080/x", "https://[::1]:8080/x"),
        // An `@` after the authority is not userinfo.
        (
            "https://host/o/r@v?x=a@b#c@d",
            "https://host/o/r@v?x=a@b#c@d",
        ),
        ("(https://u:p@a), next", "(https://a), next"),
        // Multi-byte characters right before the scheme must not land a
        // slice inside them: the scan used to panic on all three.
        (
            "\u{2018}https://u:p@h/x\u{2019}",
            "\u{2018}https://h/x\u{2019}",
        ),
        ("日本https://u:p@h/x", "日本https://h/x"),
        // `é` is not a scheme character, so the scheme is empty and,
        // being outside the ssh family, all userinfo goes.
        ("é://u:p@h/x", "é://h/x"),
        (
            "https://u:p@a?x=1 and https://v@b#frag",
            "https://a?x=1 and https://b#frag",
        ),
        (
            "https://github.com/Gaurgle/fleetz.git",
            "https://github.com/Gaurgle/fleetz.git",
        ),
        // scp-style has no `://` and no password to leak.
        (
            "git@github.com:Gaurgle/fleetz.git",
            "git@github.com:Gaurgle/fleetz.git",
        ),
        (
            "a 'https://u:p@a/x' and https://v@b/y",
            "a 'https://a/x' and https://b/y",
        ),
    ] {
        assert_eq!(redact_userinfo(url), want, "{url}");
    }
}

// --- FZ-35a (#84): reading `git fetch --progress` ---

/// Pieces of one real `git fetch --progress --prune` transcript (git
/// 2.50.1, `LC_ALL=C`, a local remote), separators and padding as git
/// wrote them.
const PROGRESS_TRANSCRIPT: &str = "remote: Enumerating objects: 198, done.        \n\
    remote: Counting objects:   0% (1/198)        \r\
    remote: Counting objects: 100% (198/198), done.        \n\
    remote: Compressing objects: 100% (122/122), done.        \n\
    Receiving objects:   0% (1/122)\r\
    Receiving objects:  71% (87/122)\r\
    remote: Total 122 (delta 76), reused 0 (delta 0), pack-reused 0 (from 0)        \n\
    Receiving objects:  72% (88/122)\r\
    Receiving objects: 100% (122/122), 175.81 KiB | 87.90 MiB/s, done.\n\
    Resolving deltas:   0% (0/76)\r\
    Resolving deltas: 100% (76/76), completed with 76 local objects.\n\
    From /tmp/gtest/up\n   425ca9a..bc60d00  main       -> origin/main\n";

fn progress(phase: FetchPhase, done: u64, total: u64, bytes: u64) -> FetchProgress {
    FetchProgress {
        phase,
        objects_done: done,
        objects_total: total,
        bytes,
    }
}

#[test]
fn a_receiving_line_gives_objects_and_bytes() {
    assert_eq!(
        parse_fetch_progress("Receiving objects:  45% (123/273), 1.23 MiB | 1.20 MiB/s"),
        Some(progress(
            FetchPhase::Receiving,
            123,
            273,
            (1.23_f64 * 1024.0 * 1024.0).round() as u64
        ))
    );
    // The final summary, `, done.` and all.
    assert_eq!(
        parse_fetch_progress("Receiving objects: 100% (122/122), 175.81 KiB | 87.90 MiB/s, done."),
        Some(progress(
            FetchPhase::Receiving,
            122,
            122,
            (175.81_f64 * 1024.0).round() as u64
        ))
    );
    // Before git has a rate to show, no size.
    assert_eq!(
        parse_fetch_progress("Receiving objects:   0% (1/122)"),
        Some(progress(FetchPhase::Receiving, 1, 122, 0))
    );
}

#[test]
fn every_size_unit_is_read() {
    for (size, bytes) in [
        ("312 bytes", 312),
        ("1 byte", 1),
        ("7 B", 7),
        ("2.00 KiB", 2048),
        ("312.00 MiB", 312 * 1024 * 1024),
        ("1.50 GiB", 3 * 512 * 1024 * 1024),
    ] {
        let line = format!("Receiving objects:  50% (5/10), {size} | 1.00 MiB/s");
        assert_eq!(
            parse_fetch_progress(&line).map(|p| p.bytes),
            Some(bytes),
            "{line}"
        );
    }
    // An unknown unit is no size, not a wrong one.
    assert_eq!(
        parse_fetch_progress("Receiving objects:  50% (5/10), 2 PiB | 1 PiB/s").map(|p| p.bytes),
        Some(0)
    );
}

#[test]
fn resolving_and_server_side_lines_are_progress() {
    assert_eq!(
        parse_fetch_progress("Resolving deltas:  12% (3/25)"),
        Some(progress(FetchPhase::Resolving, 3, 25, 0))
    );
    assert_eq!(
        parse_fetch_progress("Resolving deltas: 100% (76/76), completed with 76 local objects."),
        Some(progress(FetchPhase::Resolving, 76, 76, 0))
    );
    assert_eq!(
        parse_fetch_progress("remote: Compressing objects:  45% (55/122)        "),
        Some(progress(FetchPhase::Remote, 55, 122, 0))
    );
    assert_eq!(
        parse_fetch_progress("remote: Enumerating objects: 198, done.        "),
        Some(progress(FetchPhase::Remote, 198, 0, 0))
    );
}

#[test]
fn error_lines_are_not_progress() {
    for line in [
        "fatal: '/x/no-such-remote' does not appear to be a git repository",
        "fatal: Could not read from remote repository.",
        "ssh: connect to host github.com port 22: Operation timed out",
        "remote: error: 403 forbidden",
        "remote: fatal: 12",
        "remote: Total 122 (delta 76), reused 0 (delta 0), pack-reused 0 (from 0)",
        "Checking connectivity: 100% (3/3)",
        "From /tmp/up",
        "",
    ] {
        assert_eq!(parse_fetch_progress(line), None, "{line:?}");
    }
}

/// The stream as the watcher reads it: split on `\r` and `\n` whatever
/// the chunking, every update handed out, bytes carried through the
/// phases after receiving, and nothing kept for a fetch that worked.
#[test]
fn a_progress_transcript_streams_updates_and_keeps_nothing() {
    let mut log = FetchStderr::default();
    let mut updates = Vec::new();
    for chunk in PROGRESS_TRANSCRIPT.as_bytes().chunks(7) {
        updates.extend(log.feed(chunk));
    }
    updates.extend(log.finish());
    assert_eq!(updates.len(), 10, "{updates:#?}");
    let received = (175.81_f64 * 1024.0).round() as u64;
    assert_eq!(
        updates[8],
        progress(FetchPhase::Resolving, 0, 76, received),
        "resolving carries the received bytes"
    );
    assert_eq!(updates[4], progress(FetchPhase::Receiving, 1, 122, 0));
    assert_eq!(String::from_utf8_lossy(&log.kept), "");
}

/// Criterion 1: what is left for `classify_fetch_error` and
/// `first_error_line` is what `--quiet` left them, so a failure reads as
/// it did before progress was asked for.
#[test]
fn a_failing_transcript_keeps_its_error_lines_for_classification() {
    let failing = "remote: Enumerating objects: 3, done.        \n\
        Receiving objects:  33% (1/3)\r\
        ssh: connect to host example.com port 22: Operation timed out\r\n\
        fatal: Could not read from remote repository.\n\
        \n\
        Please make sure you have the correct access rights\n\
        and the repository exists.\n";
    let mut log = FetchStderr::default();
    let updates = log.feed(failing.as_bytes());
    assert_eq!(updates.len(), 2);
    assert_eq!(log.finish(), None);
    let kept = String::from_utf8_lossy(&log.kept).into_owned();
    assert_eq!(
        kept,
        "ssh: connect to host example.com port 22: Operation timed out\n\
         fatal: Could not read from remote repository.\n\
         Please make sure you have the correct access rights\n\
         and the repository exists.\n"
    );
    assert_eq!(classify_fetch_error(&kept), FetchFailureKind::Unreachable);
    assert_eq!(
        first_error_line(&kept).as_deref(),
        Some("ssh: connect to host example.com port 22: Operation timed out")
    );
}

/// A last piece with no line end is still read when the pipe closes.
#[test]
fn the_last_piece_is_read_at_the_end_of_the_pipe() {
    let mut log = FetchStderr::default();
    assert!(
        log.feed(b"Receiving objects:  50% (1/2), 3.00 KiB | 1 KiB/s")
            .is_empty()
    );
    assert_eq!(
        log.finish(),
        Some(progress(FetchPhase::Receiving, 1, 2, 3072))
    );
    let mut log = FetchStderr::default();
    assert!(log.feed(b"fatal: no").is_empty());
    assert_eq!(log.finish(), None);
    assert_eq!(String::from_utf8_lossy(&log.kept), "fatal: no\n");
}
