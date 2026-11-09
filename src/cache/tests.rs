use super::*;

#[test]
fn cache_path_varies_by_query_shape() {
    // `repo_limit` is the only thing left that changes what is asked for,
    // so it is the only thing the key may vary by. This used to also split
    // on `-p`, back when that flag decided whether PR data was fetched;
    // it now decides only whether PR data is drawn. Keeping the split
    // meant startup and the background poll wrote two files and each read
    // only its own, so a launch could never be warmed by the poll before
    // it.
    assert_ne!(ApiCache::new(50).path, ApiCache::new(100).path);
    assert_eq!(ApiCache::new(50).path, ApiCache::new(50).path);
}

#[test]
fn cache_write_and_read() {
    let dir = std::env::temp_dir().join("fleetz_cache_test");
    let _ = std::fs::remove_dir_all(&dir);
    let cache = ApiCache {
        path: dir.join("test.json"),
    };

    cache.write(r#"{"test": true}"#);
    let data = cache.read();
    assert!(data.is_some());
    assert!(data.unwrap().contains("test"));

    std::fs::remove_dir_all(dir).ok();
}

/// The stale read is what a fallback is built from, and the age it
/// reports has to be the file's, not the moment it was read: a cache
/// written hours ago is hours old however recently fleetz looked at it.
#[test]
fn a_stale_read_reports_when_the_file_was_written() {
    let dir = std::env::temp_dir().join(format!("fleetz_cache_written_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = ApiCache {
        path: dir.join("test.json"),
    };
    cache.write("data");
    // Planted, because a test cannot wait three hours for a file to age.
    let planted = SystemTime::now() - Duration::from_secs(3 * 3600);
    std::fs::File::options()
        .write(true)
        .open(&cache.path)
        .unwrap()
        .set_modified(planted)
        .unwrap();

    let stale = cache.read_stale().expect("the file is there");
    assert_eq!(stale.body, "data");
    let written = stale.written.expect("the write time is readable");
    // Filesystems round mtimes, so compare to the second.
    let drift = match written.duration_since(planted) {
        Ok(d) => d,
        Err(e) => e.duration(),
    };
    assert!(
        drift < Duration::from_secs(1),
        "read {written:?}, planted {planted:?}"
    );

    std::fs::remove_dir_all(dir).ok();
}

/// A fresh directory of its own under the system temp dir, removed when
/// dropped, so a failing assertion cleans up too. On Unix the drop first
/// gives the owner its permissions back, which CW6 takes away.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("fleetz_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Every name in the directory, sorted, so a test can assert that no
    /// temporary file was left beside the cache file.
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// CW3: the last write wins whole, and the file's mtime is that write's,
/// since the mtime is the data time the stale read reports. The first
/// write is aged three hours so a second write that kept the old mtime
/// cannot pass.
#[test]
fn cw3_two_writes_in_sequence_leave_the_second_body_and_its_mtime() {
    let scratch = Scratch::new("cw3");
    let cache = ApiCache::at(scratch.0.join("github-50.json"));
    cache.write("first body, the longer of the two");
    let aged = SystemTime::now() - Duration::from_secs(3 * 3600);
    std::fs::File::options()
        .write(true)
        .open(&cache.path)
        .unwrap()
        .set_modified(aged)
        .unwrap();

    let before = SystemTime::now();
    cache.write("second");
    let after = SystemTime::now();

    assert_eq!(std::fs::read_to_string(&cache.path).unwrap(), "second");
    let written = cache.written().expect("the write time is readable");
    // Filesystems round mtimes, so allow a second either side.
    assert!(
        written + Duration::from_secs(1) >= before && written <= after + Duration::from_secs(1),
        "mtime {written:?} is not the second write's ({before:?} to {after:?})"
    );
    assert_eq!(scratch.names(), ["github-50.json"]);
}

/// CW5: a prefix left by a torn write is replaced by the full body, byte
/// for byte, and nothing else is left in the directory.
#[test]
fn cw5_a_full_write_replaces_a_truncated_file_byte_for_byte() {
    let scratch = Scratch::new("cw5");
    let path = scratch.0.join("github-50.json");
    let full: String = (0..400)
        .map(|i| format!(r#"{{"name":"repo-{i}","stars":{i}}},"#))
        .collect();
    assert!(full.len() > 1000);
    std::fs::write(&path, &full.as_bytes()[..1000]).unwrap();

    ApiCache::at(path.clone()).write(&full);

    assert_eq!(std::fs::read(&path).unwrap(), full.as_bytes());
    assert_eq!(scratch.names(), ["github-50.json"]);
}

/// CW6: a directory the process cannot create files in. The old cache
/// must survive whole and the write must neither panic nor leave
/// anything behind. A plain `fs::write` truncated and rewrote the
/// existing file here, since writing to a file needs no directory
/// permission.
#[cfg(unix)]
#[test]
fn cw6_a_write_into_a_read_only_directory_leaves_the_old_file() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("cw6");
    let path = scratch.0.join("github-50.json");
    std::fs::write(&path, "old body").unwrap();
    std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root ignores directory permissions, so the scenario cannot be set
    // up; say so rather than pass without having tested anything.
    let probe = scratch.0.join("probe");
    if std::fs::File::create(&probe).is_ok() {
        let _ = std::fs::remove_file(&probe);
        eprintln!("SKIPPED cw6: the directory is writable anyway (running as root?)");
        return;
    }

    ApiCache::at(path.clone()).write("new body");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old body");
    assert_eq!(scratch.names(), ["github-50.json"]);
}

/// CW8: the saved copy is readable only by its owner (O14). Masked by
/// the umask like any created file, so it holds for every umask that
/// leaves the owner's read and write bits alone.
#[cfg(unix)]
#[test]
fn cw8_a_write_into_an_empty_directory_gives_mode_0600() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("cw8");
    let path = scratch.0.join("github-50.json");

    ApiCache::at(path.clone()).write("body");

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "mode was {mode:o}");
}

/// The rename puts a new inode in place, so a cache file saved before
/// O14 at 0644 is 0600 after its next write rather than keeping its mode.
#[cfg(unix)]
#[test]
fn a_replaced_0644_cache_file_ends_up_0600() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("mode_replace");
    let path = scratch.0.join("github-50.json");
    std::fs::write(&path, "old body").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    ApiCache::at(path.clone()).write("new body");

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "mode was {mode:o}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new body");
}

/// CW1: the rename fails after the new body is fully written. The old
/// body stays and the temporary file is removed. The temporary file is
/// beside the target and named as FZ-09c's cleanup expects.
#[test]
fn cw1_a_failed_rename_leaves_the_old_body_and_no_temporary_file() {
    let scratch = Scratch::new("cw1");
    let target = scratch.0.join("github-50.json");
    std::fs::write(&target, "body W").unwrap();
    let temp = temp_path_for(&target).unwrap();
    let prefix = format!(".github-50.json.tmp-{}-", std::process::id());
    let temp_name = temp.file_name().unwrap().to_string_lossy().into_owned();
    assert!(temp_name.starts_with(&prefix), "temporary name {temp_name}");
    assert_eq!(temp.parent(), target.parent());

    let mut renamed_from = None;
    let result = replace_atomically(
        &target,
        &temp,
        |file| file.write_all(b"body X"),
        |from, _| {
            assert_eq!(std::fs::read_to_string(from).unwrap(), "body X");
            renamed_from = Some(from.to_path_buf());
            Err(io::Error::other("injected rename failure"))
        },
    );

    assert!(result.is_err());
    assert_eq!(renamed_from.as_deref(), Some(temp.as_path()));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "body W");
    assert_eq!(scratch.names(), ["github-50.json"]);
}

/// The write itself fails part-way (a full disk): the half-written
/// temporary file is removed, the rename never runs, the old body stays.
#[test]
fn a_write_that_fails_part_way_leaves_the_old_body_and_no_temporary_file() {
    let scratch = Scratch::new("partial");
    let target = scratch.0.join("github-50.json");
    std::fs::write(&target, "body W").unwrap();
    let temp = temp_path_for(&target).unwrap();

    let result = replace_atomically(
        &target,
        &temp,
        |file| {
            file.write_all(b"half of X")?;
            Err(io::Error::other("injected write failure"))
        },
        |_, _| panic!("a failed write must not be renamed into place"),
    );

    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "body W");
    assert_eq!(scratch.names(), ["github-50.json"]);
}

/// A file already at the temporary name (another process's, or a
/// crash's) is neither reused nor deleted: `create_new` refuses it and
/// the write is skipped with the old body in place.
#[test]
fn a_taken_temporary_name_skips_the_write_and_keeps_both_files() {
    let scratch = Scratch::new("taken");
    let target = scratch.0.join("github-50.json");
    std::fs::write(&target, "body W").unwrap();
    let temp = temp_path_for(&target).unwrap();
    std::fs::write(&temp, "someone else's").unwrap();

    let result = replace_atomically(
        &target,
        &temp,
        |_| panic!("a taken name must not be written"),
        |_, _| panic!("a taken name must not be renamed"),
    );

    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "body W");
    assert_eq!(std::fs::read_to_string(&temp).unwrap(), "someone else's");
}

/// CW7: `R` and a background poll write at once in one process. Writer 1
/// fills its temporary file; writer 2 then creates its own and renames;
/// writer 1 renames last. Built through the rename seam, not threads, so
/// the interleaving is the one named. With a shared name writer 2 would
/// have clobbered or renamed away writer 1's file.
#[test]
fn cw7_two_interleaved_writes_in_one_process_leave_writer_ones_body() {
    let scratch = Scratch::new("cw7");
    let target = scratch.0.join("github-50.json");
    let body_1 = "writer 1's complete body, longer than writer 2's";
    let body_2 = "writer 2's body";
    let temp_1 = temp_path_for(&target).unwrap();
    let temp_2 = temp_path_for(&target).unwrap();
    assert_ne!(temp_1, temp_2, "two writes shared a temporary name");

    let result = replace_atomically(
        &target,
        &temp_1,
        |file| file.write_all(body_1.as_bytes()),
        |from, to| {
            replace_atomically(
                &target,
                &temp_2,
                |file| file.write_all(body_2.as_bytes()),
                |from, to| std::fs::rename(from, to),
            )?;
            assert_eq!(std::fs::read_to_string(&target).unwrap(), body_2);
            assert_eq!(std::fs::read_to_string(from).unwrap(), body_1);
            std::fs::rename(from, to)
        },
    );

    assert!(result.is_ok());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), body_1);
    assert_eq!(scratch.names(), ["github-50.json"]);
}

// --- FZ-09c: the keyed file, the launch read order and cleanup ---

/// Dummy tokens. A is the design's example token, B the same shape.
const TOKEN_A: &str = "gho_EXAMPLEexampleEXAMPLEexample0000";
const TOKEN_B: &str = "gho_EXAMPLEexampleEXAMPLEexample1111";
const DAY: Duration = Duration::from_secs(24 * 3600);

fn account(token: &str) -> String {
    crate::github::token_fingerprint(token)
}

/// Writes `body` at `path` and dates it `age` ago.
fn plant(path: &Path, body: &str, age: Duration) {
    std::fs::write(path, body).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

fn keyed_at(scratch: &Scratch, version: u32, account: &str, repo_limit: u32) -> PathBuf {
    scratch
        .0
        .join(keyed_name(version, "github.com", account, repo_limit))
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

/// CK1: two tokens, one `repo_limit`, two files; writing B's leaves A's
/// byte for byte.
#[test]
fn ck1_two_tokens_get_two_files_and_writing_one_leaves_the_other() {
    let scratch = Scratch::new("ck1");
    let cache = KeyedCache::at(scratch.0.clone(), 50, "github.com");
    let file_a = cache.for_account(&account(TOKEN_A));
    let file_b = cache.for_account(&account(TOKEN_B));
    assert_ne!(file_a.path(), file_b.path());

    file_a.write("account A's body");
    let before = std::fs::read(file_a.path()).unwrap();
    file_b.write("account B's body, longer");

    assert_eq!(std::fs::read(file_a.path()).unwrap(), before);
    assert_eq!(
        std::fs::read_to_string(file_b.path()).unwrap(),
        "account B's body, longer"
    );
}

/// CK3: one token, `repo_limit` 50 and 100, two files.
#[test]
fn ck3_one_token_two_repo_limits_get_two_files() {
    let dir = std::env::temp_dir().join("fleetz_ck3_unused");
    let at_50 = KeyedCache::at(dir.clone(), 50, "github.com");
    let at_100 = KeyedCache::at(dir, 100, "github.com");
    let a = account(TOKEN_A);
    assert_ne!(at_50.for_account(&a).path(), at_100.for_account(&a).path());
    assert!(file_name(at_50.for_account(&a).path()).ends_with("-50.json"));
    assert!(file_name(at_100.for_account(&a).path()).ends_with("-100.json"));
}

/// CK5: the host part is the host key `RemoteRef` compares a github.com
/// remote by, whatever case the remote was written in.
#[test]
fn ck5_the_host_part_is_the_remote_ref_host_key() {
    for url in [
        "git@github.com:Gaurgle/fleetz.git",
        "https://GitHub.com/Gaurgle/fleetz",
    ] {
        let remote = crate::model::RemoteRef::parse(url).unwrap();
        assert_eq!(host_part(&remote.host), remote.key().host, "{url}");
    }
    assert_eq!(host_part(crate::github::HOST), "github.com");
}

/// A host that is not plain `[a-z0-9.-]` is hashed, never written raw:
/// nothing it holds reaches the file name.
#[test]
fn a_host_with_other_characters_is_hashed() {
    for host in ["git hub.com", "evil/../x", "a_b.com", "", "[::1]"] {
        let part = host_part(host);
        assert!(part.starts_with("h_"), "{host:?} gave {part:?}");
        assert!(is_lower_hex_16(&part[2..]), "{host:?} gave {part:?}");
        assert!(is_host_part(&part));
    }
    assert_ne!(host_part("a_b.com"), host_part("a_c.com"));
    // Case folds before hashing, as it does for a plain host.
    assert_eq!(host_part("A_B.com"), host_part("a_b.com"));
    assert_eq!(host_part("ghe.corp-1.example"), "ghe.corp-1.example");
}

/// CK6: the name carries the pinned fingerprint, bare or padded token, and
/// the empty token's; the design's example name exactly.
#[test]
fn ck6_the_file_name_carries_the_pinned_fingerprint() {
    let cache = KeyedCache::at(PathBuf::from("/nonexistent"), 50, "github.com");
    let named = |token: &str| file_name(cache.for_account(&account(token)).path());
    assert_eq!(
        named(TOKEN_A),
        "github-v2-github.com-2a8d4ab865f3ca20-50.json"
    );
    assert_eq!(
        named(&format!("  {TOKEN_A}\n")),
        "github-v2-github.com-2a8d4ab865f3ca20-50.json"
    );
    assert_eq!(named(""), "github-v2-github.com-5fccb9a3d0a45785-50.json");
}

/// A keyed name parses back into its parts, a host with dashes included,
/// and near misses do not parse.
#[test]
fn keyed_names_parse_exactly() {
    let a = account(TOKEN_A);
    let name = keyed_name(2, "ghe-1.example", &a, 50);
    assert_eq!(
        parse_keyed_name(&name),
        Some(KeyedName {
            version: 2,
            host: "ghe-1.example",
            account: &a,
            repo_limit: 50
        })
    );
    for near_miss in [
        format!("github-v2-github.com-{a}-50.json.bak"),
        format!("github-v2-github.com-{a}-+50.json"),
        format!("github-v-github.com-{a}-50.json"),
        "github-v2-github.com-2A8D4AB865F3CA20-50.json".to_string(),
        "github-v2-github.com-2a8d4ab865f3ca2-50.json".to_string(),
        format!("github-v2-Git_Hub-{a}-50.json"),
        format!("github-v2-{a}-50.json"),
    ] {
        assert_eq!(parse_keyed_name(&near_miss), None, "{near_miss}");
    }
}

/// CK9: `ApiCache::new`'s deletions are today's three obsolete names and
/// nothing else. Driven through the helper it calls, in a scratch
/// directory, since `new` itself reaches the real one.
#[test]
fn ck9_api_cache_new_removes_only_the_three_obsolete_names() {
    let scratch = Scratch::new("ck9");
    let a = account(TOKEN_A);
    let kept = [
        "github-50.json".to_string(),
        "github-100-prs.json".to_string(),
        keyed_name(FORMAT_VERSION, "github.com", &a, 50),
        keyed_name(1, "github.com", &a, 50),
        ".github-50.json.tmp-1-1".to_string(),
        "window.toml".to_string(),
    ];
    for name in kept.iter().map(String::as_str).chain([
        "github.json",
        "github-50-prs.json",
        "github-50-noprs.json",
    ]) {
        plant(&scratch.0.join(name), "x", 40 * DAY);
    }

    remove_obsolete_names(&scratch.0, 50);

    let mut expected = kept.to_vec();
    expected.sort();
    assert_eq!(scratch.names(), expected);
}

/// What `launch_reads` offers, as file names and whether each is the
/// credential's own.
fn offered(cache: &KeyedCache, account: Option<&str>) -> Vec<(String, bool)> {
    cache
        .launch_reads(account)
        .into_iter()
        .map(|read| (file_name(read.cache.path()), read.own))
        .collect()
}

/// Step 1, then step 2 only while no account has a current keyed file.
#[test]
fn launch_offers_the_own_file_then_the_legacy_one_only_while_nothing_is_keyed() {
    let scratch = Scratch::new("reads_known");
    let cache = KeyedCache::at(scratch.0.clone(), 50, "github.com");
    let (a, b) = (account(TOKEN_A), account(TOKEN_B));
    let own = keyed_name(FORMAT_VERSION, "github.com", &a, 50);
    plant(&scratch.0.join("github-50.json"), "legacy", DAY);

    // Nothing keyed: own, then legacy.
    assert_eq!(
        offered(&cache, Some(&a)),
        [(own.clone(), true), ("github-50.json".to_string(), false)]
    );
    // An older version, another limit or another host is not "keyed" here.
    plant(&keyed_at(&scratch, 1, &b, 50), "v1", DAY);
    plant(&keyed_at(&scratch, FORMAT_VERSION, &b, 100), "100", DAY);
    plant(
        &scratch
            .0
            .join(keyed_name(FORMAT_VERSION, "ghe.example", &b, 50)),
        "ghe",
        DAY,
    );
    assert_eq!(offered(&cache, Some(&a)).len(), 2);
    // Another account's current file ends the legacy step.
    plant(&keyed_at(&scratch, FORMAT_VERSION, &b, 50), "b", DAY);
    assert_eq!(offered(&cache, Some(&a)), [(own, true)]);
}

/// Signed out: the newest current keyed file of any account, else the
/// legacy file, never both.
#[test]
fn launch_signed_out_offers_the_newest_keyed_file_or_else_the_legacy_one() {
    let scratch = Scratch::new("reads_unknown");
    let cache = KeyedCache::at(scratch.0.clone(), 50, "github.com");
    let (a, b) = (account(TOKEN_A), account(TOKEN_B));
    assert_eq!(
        offered(&cache, None),
        [("github-50.json".to_string(), false)]
    );

    plant(&keyed_at(&scratch, FORMAT_VERSION, &a, 50), "a", 2 * DAY);
    plant(&keyed_at(&scratch, FORMAT_VERSION, &b, 50), "b", DAY);
    plant(&scratch.0.join("github-50.json"), "legacy", Duration::ZERO);
    assert_eq!(
        offered(&cache, None),
        [(keyed_name(FORMAT_VERSION, "github.com", &b, 50), false)]
    );
}

/// Every kind of superseded file, each past 30 days, beside the current
/// account's current file: the positive control the rule tests below
/// each break one condition of.
fn plant_superseded(scratch: &Scratch) -> (PathBuf, Vec<PathBuf>) {
    let (a, b) = (account(TOKEN_A), account(TOKEN_B));
    let current = keyed_at(scratch, FORMAT_VERSION, &a, 50);
    plant(&current, "mine", Duration::ZERO);
    let superseded = vec![
        scratch.0.join("github-50.json"),
        keyed_at(scratch, 1, &a, 50),
        keyed_at(scratch, FORMAT_VERSION, &b, 50),
    ];
    for path in &superseded {
        plant(path, "old", 40 * DAY);
    }
    (current, superseded)
}

fn remove_now(dir: Option<&Path>, account: Option<&str>, served: Option<&Path>) {
    remove_superseded_in(dir, "github.com", account, served, SystemTime::now());
}

#[test]
fn superseded_files_go_when_every_rule_holds() {
    let scratch = Scratch::new("sup_all");
    let (current, _) = plant_superseded(&scratch);
    remove_now(Some(&scratch.0), Some(&account(TOKEN_A)), None);
    assert_eq!(scratch.names(), [file_name(&current)]);
}

/// CM7: signed out, or the lookup timed out: no account, nothing goes.
#[test]
fn cm7_without_a_known_account_nothing_is_deleted() {
    let scratch = Scratch::new("cm7");
    plant_superseded(&scratch);
    let before = scratch.names();
    remove_now(Some(&scratch.0), None, None);
    assert_eq!(scratch.names(), before);
}

/// CM8: the file this launch served stays, even matching every other rule.
#[test]
fn cm8_the_file_just_served_is_not_deleted() {
    let scratch = Scratch::new("cm8");
    let (current, superseded) = plant_superseded(&scratch);
    let served = &superseded[0];
    remove_now(Some(&scratch.0), Some(&account(TOKEN_A)), Some(served));
    let mut expected = vec![file_name(&current), file_name(served)];
    expected.sort();
    assert_eq!(scratch.names(), expected);
}

/// CM9: names outside the explicit patterns are never touched, however
/// old: `window.toml` lives here.
#[test]
fn cm9_unrelated_files_are_never_deleted() {
    let scratch = Scratch::new("cm9");
    let a = account(TOKEN_A);
    plant(
        &keyed_at(&scratch, FORMAT_VERSION, &a, 50),
        "mine",
        Duration::ZERO,
    );
    for name in [
        "window.toml",
        "notes.json",
        "github-50.json.bak",
        ".github-50.json.tmp-1-1",
    ] {
        plant(&scratch.0.join(name), "keep", 40 * DAY);
    }
    let before = scratch.names();
    remove_now(Some(&scratch.0), Some(&a), None);
    assert_eq!(scratch.names(), before);
}

/// A newer binary's version, another host's file and the account's own
/// current file are not superseded either.
#[test]
fn newer_versions_other_hosts_and_the_own_file_stay() {
    let scratch = Scratch::new("sup_newer");
    let (a, b) = (account(TOKEN_A), account(TOKEN_B));
    plant(
        &keyed_at(&scratch, FORMAT_VERSION, &a, 50),
        "mine",
        40 * DAY,
    );
    plant(
        &keyed_at(&scratch, FORMAT_VERSION + 1, &b, 50),
        "v3",
        40 * DAY,
    );
    plant(
        &scratch.0.join(keyed_name(1, "ghe.example", &a, 50)),
        "ghe",
        40 * DAY,
    );
    let before = scratch.names();
    remove_now(Some(&scratch.0), Some(&a), None);
    assert_eq!(scratch.names(), before);
}

/// CM10: no platform cache directory, no cleanup.
#[test]
fn cm10_without_a_cache_directory_cleanup_is_skipped() {
    let scratch = Scratch::new("cm10");
    plant_superseded(&scratch);
    let before = scratch.names();
    remove_now(None, Some(&account(TOKEN_A)), None);
    assert_eq!(scratch.names(), before);
}

/// A superseded file written within 30 days stays; at 30 days it goes.
#[test]
fn a_superseded_file_written_within_30_days_stays() {
    let scratch = Scratch::new("sup_age");
    let a = account(TOKEN_A);
    let current = keyed_at(&scratch, FORMAT_VERSION, &a, 50);
    plant(&current, "mine", Duration::ZERO);
    let recent = keyed_at(&scratch, 1, &a, 50);
    plant(&recent, "29 days", 29 * DAY);
    let legacy = scratch.0.join("github-50.json");
    plant(&legacy, "30 days", 30 * DAY);

    remove_now(Some(&scratch.0), Some(&a), None);

    let mut expected = vec![file_name(&current), file_name(&recent)];
    expected.sort();
    assert_eq!(scratch.names(), expected);
}

/// The current account's current file must exist for the superseded
/// file's own `repo_limit`, not just for some other one.
#[test]
fn a_superseded_file_stays_until_its_limit_has_a_current_file() {
    let scratch = Scratch::new("sup_limit");
    let a = account(TOKEN_A);
    let current_100 = keyed_at(&scratch, FORMAT_VERSION, &a, 100);
    plant(&current_100, "mine at 100", Duration::ZERO);
    let legacy_50 = scratch.0.join("github-50.json");
    plant(&legacy_50, "old", 40 * DAY);
    plant(&scratch.0.join("github-100.json"), "old", 40 * DAY);

    remove_now(Some(&scratch.0), Some(&a), None);

    let mut expected = vec![file_name(&current_100), file_name(&legacy_50)];
    expected.sort();
    assert_eq!(scratch.names(), expected);
}

/// CW2: a crash's temporary file over an hour old goes; a 10 minute old
/// one, possibly a live write, stays, and so does anything not named as
/// a cache file's temporary.
#[test]
fn cw2_startup_removes_temporary_files_older_than_an_hour() {
    let scratch = Scratch::new("cw2");
    let a = account(TOKEN_A);
    let abandoned = format!(".{}.tmp-4242-7", keyed_name(2, "github.com", &a, 50));
    plant(
        &scratch.0.join(&abandoned),
        "half",
        Duration::from_secs(2 * 3600),
    );
    plant(
        &scratch.0.join(".github-50.json.tmp-4242-8"),
        "live",
        Duration::from_secs(600),
    );
    for name in [
        ".window.toml.tmp-1-1",
        ".github-50.json.tmp-x-1",
        ".github-50.json.tmp-1",
        "github-50.json",
        "notes.json",
    ] {
        plant(&scratch.0.join(name), "keep", Duration::from_secs(2 * 3600));
    }
    let mut expected: Vec<String> = scratch
        .names()
        .into_iter()
        .filter(|name| *name != abandoned)
        .collect();
    expected.sort();

    remove_abandoned_temp_files_in(Some(&scratch.0), SystemTime::now());

    assert_eq!(scratch.names(), expected);
    remove_abandoned_temp_files_in(None, SystemTime::now());
}

/// A temporary file `write` itself would leave is the pattern startup
/// looks for.
#[test]
fn the_temporary_names_writes_use_are_the_ones_cleanup_matches() {
    let cache = KeyedCache::at(PathBuf::from("/nonexistent"), 50, "github.com");
    let target = cache.for_account(&account(TOKEN_A)).path().to_path_buf();
    let temp = temp_path_for(&target).unwrap();
    assert!(is_cache_temp_name(&file_name(&temp)));
    assert!(is_cache_temp_name(&file_name(
        &temp_path_for(Path::new("/x/github-50.json")).unwrap()
    )));
}
