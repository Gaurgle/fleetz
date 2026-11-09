//! The golden harness for print mode (FZ-46, D2 of
//! `docs/repoz-print-design.md`): for one fixture filesystem, one set of
//! remotes and one recorded GitHub payload, print mode must write the bytes
//! bash repoz writes, except the version line.
//!
//! [`Fixture::build`] is the shared fixture every report test starts from: a
//! temp `HOME` with two scan roots, bare remotes on disk (reached through a
//! `url.insteadOf` rewrite, so origins read as GitHub URLs and fetches work
//! offline), and `fixtures/graphql.json` standing in for GitHub. It covers a
//! user repo and an org repo cloned under the roots, a repo cloned elsewhere
//! under `HOME` (bash's `also active`) and a repo that is not cloned. It is
//! built so the D3 differences cannot arise: one clone per repo, origins that
//! agree with folder names and owners, no archived repos, every clone fetched
//! (so `FETCH_HEAD` exists), and the behind and ahead counts already in local
//! refs, so the result does not depend on which clones a tool fetches.
//!
//! Bash repoz's output for each case in [`CASES`] is recorded beside the
//! payload as `fixtures/<case>.bash.out`. `bash_repoz_renders_the_fixture`
//! re-runs bash repoz through its fixture hook (prerequisite P1:
//! `REPOZ_GRAPHQL_FIXTURE`, `REPOZ_GH_LOGIN`) and fails when the recording
//! no longer matches, leaving the live output in the temp directory to copy
//! over it. The golden test compares print mode with the recording, so it
//! runs where bash repoz is not installed.
//!
//! Nothing here depends on the clock (D9: `-a` and absolute `--since` dates,
//! fixed commit dates, `TZ=UTC`) or on the terminal: both tools run with
//! `COLUMNS=100`, a UTF-8 locale and no controlling terminal, since bash
//! repoz asks `stty size </dev/tty` before it reads `COLUMNS`.

use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The GitHub login the payload belongs to, and the owner of its user repos.
pub const LOGIN: &str = "fixture-user";
/// The organization owning [`Fixture::org_clone`].
pub const ORG: &str = "fixture-org";
/// In the payload, cloned nowhere.
pub const NOT_CLONED: &str = "ghost-repo";
/// The terminal width both tools are run at.
pub const COLUMNS: &str = "100";
/// The locale both tools are run under. bash measures `${#status}` in
/// characters only under a UTF-8 locale; under `C` it counts the bytes of
/// `↓` and the dotted leaders come out short.
pub const LOCALE: &str = "en_US.UTF-8";

/// The recorded cases: a name for `fixtures/<name>.bash.out` and the repoz
/// arguments it was recorded with.
pub const CASES: &[(&str, &[&str])] = &[("all", &["-a"]), ("since", &["--since", "2026-03-01"])];

/// The `--before` cases, recorded the same way. Kept apart from [`CASES`]
/// because `--before 2026-03-11` drops `alpha-tool`, which the checks on
/// [`CASES`] require; their own tests below check them.
pub const BEFORE_CASES: &[(&str, &[&str])] = &[
    ("all-before", &["-a", "--before", "2026-03-11"]),
    (
        "since-before",
        &["--since", "2026-03-01", "--before", "2026-03-11"],
    ),
];

/// The cases recorded on the fixture with [`Fixture::add_local_changes`]:
/// bash's pass 3, which lists a local repo whatever the window.
pub const LOCAL_CASES: &[(&str, &[&str])] = &[("since-local", &["--since", "2026-03-01"])];

/// The repos [`Fixture::add_local_changes`] adds under `repo_dir`, none of
/// them in the payload: uncommitted work dated before every window, a clean
/// tree with one stash, and a clean tree stopped mid-rebase.
pub const LOCAL_REPOS: [&str; 3] = ["olddirty", "stashonly", "midrebase"];

/// The cases bash prints with no GitHub call and no summary, so no version
/// line: `-d` and the single-repo target, on the plain fixture. They compare
/// whole. `target-login` has one segment, resolved against [`LOGIN`]
/// (`REPOZ_GH_LOGIN`); `target-unknown` names a repo the payload does not
/// hold, which bash never reads in this mode.
pub const UNVERSIONED_CASES: &[(&str, &[&str])] = &[
    ("dirty-clean", &["-d"]),
    ("target-cloned", &["/fixture-user/alpha-tool"]),
    ("target-login", &["/alpha-tool"]),
    ("target-elsewhere", &["/elsewhere-app"]),
    ("target-not-cloned", &["/fixture-user/ghost-repo"]),
    ("target-unknown", &["/fixture-user/no-such-repo"]),
];

/// The cases recorded on the fixture with [`Fixture::add_worktree_and_prefix`],
/// and whether bash prints a version line for them (only the GitHub modes
/// do).
pub const WORKTREE_CASES: &[(&str, &[&str], bool)] = &[
    ("worktree-dirty", &["-d"], false),
    ("worktree-all", &["-a"], true),
    ("worktree-target", &["/fixture-user/wtmain"], false),
];

/// What [`Fixture::add_worktree_and_prefix`] adds under `repo_dir`, none of
/// it in the payload: a dirty remote-less repo whose path is a prefix of
/// `alpha-tool`'s, a clone with an upstream, a stash and an unpushed side
/// branch, and a linked worktree of that clone beside it, one unpushed
/// commit on its own branch.
pub const PREFIX_REPO: &str = "alpha";
pub const WT_MAIN: &str = "wtmain";
pub const WT_LINKED: &str = "wtmain-wt";

/// The cases recorded on the options fixture (`options_fixture` in the
/// tests: [`Fixture::add_local_changes`], [`Fixture::add_releases`],
/// [`Fixture::add_other_author`] and [`Fixture::add_pull_requests`]), and
/// whether bash prints a version line for them. Each print option alone
/// under `-a`, `-s -r -p` together, and `--no-glyphs` in the two modes that
/// ask GitHub nothing, where bash ignores the other options.
pub const OPTION_CASES: &[(&str, &[&str], bool)] = &[
    ("opt-stat", &["-a", "-s"], true),
    ("opt-releases", &["-a", "-r"], true),
    ("opt-prs", &["-a", "-p"], true),
    ("opt-author", &["-a", "--author", OTHER_AUTHOR_MATCH], true),
    ("opt-no-glyphs", &["-a", "--no-glyphs"], true),
    ("opt-stat-releases-prs", &["-a", "-s", "-r", "-p"], true),
    ("opt-dirty-no-glyphs", &["-d", "--no-glyphs"], false),
    (
        "opt-target-no-glyphs",
        &["/fixture-user/alpha-tool", "--no-glyphs"],
        false,
    ),
    (
        "opt-target-options",
        &["/fixture-user/alpha-tool", "-s", "-r", "-p"],
        false,
    ),
];

/// The cases recorded on the fixture with [`Fixture::add_missing_remotes`],
/// and whether bash prints a version line for them: the GitHub mode, where
/// both failed fetches are rows and the summary counts them, and the target
/// mode for the clone that is otherwise synced.
pub const FETCH_FAILED_CASES: &[(&str, &[&str], bool)] = &[
    ("fetch-failed-all", &["-a"], true),
    ("fetch-failed-target", &["/fixture-org/orgproj"], false),
];

/// The author [`Fixture::add_other_author`] commits as, and the `--author`
/// pattern that picks out their commits.
pub const OTHER_AUTHOR: &str = "Other Person";
pub const OTHER_AUTHOR_MATCH: &str = "Other";

const CHILD_OUT: &str = "FLEETZ_PRINT_CHILD_OUT";
const CHILD_ARGS: &str = "FLEETZ_PRINT_CHILD_ARGS";
const CHILD_TEST: &str = "report::parity::print_mode_child";

/// The fixture tree. Removed from disk on drop.
pub struct Fixture {
    /// Canonical temp root holding `home` and `remotes`. Canonical because
    /// bash repoz `realpath`s its roots and abbreviates paths under `$HOME`
    /// to `~` only when the spellings agree (`/var` is `/private/var` on
    /// macOS).
    pub root: PathBuf,
    pub home: PathBuf,
    /// `REPO_CHECK_DIR` and fleetz's `repo_dir`.
    pub repo_dir: PathBuf,
    /// bash's `EXTRA_DIRS` and fleetz's `extra_dirs`.
    pub extra_dir: PathBuf,
    /// The bare repositories the clones fetch from, outside `HOME`.
    pub remotes: PathBuf,
    /// The recorded GraphQL payload both tools read.
    pub payload: PathBuf,
    /// `alpha-tool`, private, owned by [`LOGIN`], under `repo_dir`: one
    /// commit behind its upstream.
    pub user_clone: PathBuf,
    /// `orgproj`, owned by [`ORG`], under `extra_dir`: synced.
    pub org_clone: PathBuf,
    /// `elsewhere-app`, public, owned by [`LOGIN`], under `HOME` but outside
    /// both roots: one unpushed commit.
    pub elsewhere_clone: PathBuf,
}

impl Fixture {
    /// Build the tree under a temp directory named for `tag`, which must be
    /// unique among tests that run at the same time.
    pub fn build(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!("fleetz_report_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let root = std::fs::canonicalize(&base).unwrap();
        let home = root.join("home");
        let fixture = Self {
            repo_dir: home.join("repos"),
            extra_dir: home.join("extra"),
            remotes: root.join("remotes"),
            payload: Path::new(env!("CARGO_MANIFEST_DIR")).join("src/report/fixtures/graphql.json"),
            user_clone: home.join("repos/alpha-tool"),
            org_clone: home.join("extra/orgproj"),
            elsewhere_clone: home.join("side/elsewhere-app"),
            home,
            root,
        };
        fixture.write_configs();

        fixture.publish(LOGIN, "alpha-tool", "2026-02-02T09:00:00Z");
        fixture.clone_into(LOGIN, "alpha-tool", &fixture.user_clone);
        // Pushed after the clone and fetched by it, so the clone is behind
        // before either tool runs and stays behind whether or not it fetches.
        let seed = fixture.seed("alpha-tool");
        fixture.commit(
            &seed,
            "parser.txt",
            "Add the parser",
            "2026-03-12T09:00:00Z",
        );
        fixture.git(&seed, &["push", "-q", "origin", "main"]);
        fixture.git(&fixture.user_clone, &["fetch", "-q"]);

        fixture.publish(ORG, "orgproj", "2026-03-11T09:00:00Z");
        fixture.clone_into(ORG, "orgproj", &fixture.org_clone);

        fixture.publish(LOGIN, "elsewhere-app", "2026-03-04T09:00:00Z");
        fixture.clone_into(LOGIN, "elsewhere-app", &fixture.elsewhere_clone);
        fixture.commit(
            &fixture.elsewhere_clone,
            "notes.txt",
            "Draft the notes",
            "2026-03-06T09:00:00Z",
        );

        std::fs::remove_dir_all(fixture.root.join("seed")).unwrap();
        fixture
    }

    /// The environment both tools run under, and nothing else: an inherited
    /// `GIT_DIR`, `XDG_CONFIG_HOME` or user config would aim them at the
    /// real machine.
    pub fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            ("PATH", std::env::var_os("PATH").unwrap_or_default()),
            ("HOME", self.home.clone().into_os_string()),
            ("TZ", "UTC".into()),
            ("LC_ALL", LOCALE.into()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("COLUMNS", COLUMNS.into()),
            ("REPO_CHECK_DIR", self.repo_dir.clone().into_os_string()),
            (
                "REPOZ_GRAPHQL_FIXTURE",
                self.payload.clone().into_os_string(),
            ),
            ("REPOZ_GH_LOGIN", LOGIN.into()),
        ]
    }

    /// `program` set up to run against the fixture: [`Fixture::env`] only, in
    /// `root` (outside every repository, since bash repoz adds the working
    /// directory's git toplevel to its scan), in a session of its own so
    /// `/dev/tty` cannot be opened and `COLUMNS` decides the width.
    pub fn command(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear().envs(self.env()).current_dir(&self.root);
        // SAFETY: `setsid` is async-signal-safe and touches no memory of ours.
        unsafe {
            cmd.pre_exec(|| match libc::setsid() {
                -1 => Err(io::Error::last_os_error()),
                _ => Ok(()),
            });
        }
        cmd
    }

    /// Bash repoz's stdout for `args`, raw.
    pub fn run_bash(&self, bin: &Path, args: &[&str]) -> Vec<u8> {
        let out = self.command(bin).args(args).output().expect("run repoz");
        assert!(
            out.status.success(),
            "repoz {args:?} failed: {}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    /// Print mode's output for `args`, raw. Runs `report::run` in a child
    /// copy of this test binary (`print_mode_child`), so `HOME` and the rest
    /// of [`Fixture::env`] are the child's process environment, as they are
    /// for the real binary, and no test here changes its own.
    pub fn run_print_mode(&self, args: &[&str]) -> Vec<u8> {
        self.run_print_mode_full(args).0
    }

    /// [`Fixture::run_print_mode`], with the child's own process output
    /// beside what print mode wrote.
    pub fn run_print_mode_full(&self, args: &[&str]) -> (Vec<u8>, std::process::Output) {
        self.run_print_mode_from(&self.root, args)
    }

    /// [`Fixture::run_print_mode_full`] with the child's working directory
    /// at `cwd` instead of `root`.
    pub fn run_print_mode_from(
        &self,
        cwd: &Path,
        args: &[&str],
    ) -> (Vec<u8>, std::process::Output) {
        self.run_print_mode_with(cwd, &[], args)
    }

    /// [`Fixture::run_print_mode_from`] with `env` set over
    /// [`Fixture::env`] in the child.
    pub fn run_print_mode_with(
        &self,
        cwd: &Path,
        env: &[(&str, &std::ffi::OsStr)],
        args: &[&str],
    ) -> (Vec<u8>, std::process::Output) {
        let out_path = self.root.join("print-mode.out");
        let _ = std::fs::remove_file(&out_path);
        let exe = std::env::current_exe().expect("test binary");
        let out = self
            .command(&exe)
            .envs(env.iter().copied())
            .current_dir(cwd)
            .args([CHILD_TEST, "--exact", "--quiet", "--test-threads=1"])
            .env(CHILD_OUT, &out_path)
            .env(CHILD_ARGS, args.join("\n"))
            .output()
            .expect("run print mode");
        assert!(
            out.status.success(),
            "print mode {args:?} failed: {}\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let report = std::fs::read(&out_path).expect("print mode wrote no output file");
        (report, out)
    }

    /// One more commit on `owner/name`'s remote, pushed from a scratch clone
    /// so no fixture clone sees it until it fetches.
    pub fn push_upstream(&self, owner: &str, name: &str, subject: &str) {
        let scratch = self.seed(name);
        self.git(
            &self.root,
            &[
                "clone",
                "-q",
                &github_url(owner, name),
                scratch.to_str().unwrap(),
            ],
        );
        self.commit(&scratch, "upstream.txt", subject, "2026-03-13T09:00:00Z");
        self.git(&scratch, &["push", "-q", "origin", "main"]);
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    /// The [`LOCAL_REPOS`], remote-less, under `repo_dir`: what bash's pass 3
    /// lists and a window must not hide. `olddirty`'s last commit and its
    /// uncommitted edit are both dated 2026-01-05, before every window;
    /// `stashonly` is clean with one stash; `midrebase` is clean, stopped by
    /// a failing `--exec` with its `rebase-merge` state on disk.
    pub fn add_local_changes(&self) {
        let [olddirty, stashonly, midrebase] = LOCAL_REPOS.map(|name| self.repo_dir.join(name));

        self.init(&olddirty);
        self.commit(&olddirty, "work.txt", "Old work", "2026-01-05T09:00:00Z");
        let edited = olddirty.join("work.txt");
        std::fs::write(&edited, "Old work, edited\n").unwrap();
        let old = chrono::DateTime::parse_from_rfc3339("2026-01-05T10:00:00Z").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&edited)
            .unwrap()
            .set_modified(old.into())
            .unwrap();

        self.init(&stashonly);
        self.commit(&stashonly, "draft.txt", "Draft", "2026-01-06T09:00:00Z");
        std::fs::write(stashonly.join("draft.txt"), "Draft, put aside\n").unwrap();
        self.git(&stashonly, &["stash", "-q"]);

        self.init(&midrebase);
        self.commit(&midrebase, "a.txt", "First", "2026-01-07T09:00:00Z");
        self.commit(&midrebase, "b.txt", "Second", "2026-01-08T09:00:00Z");
        // Stops after picking `Second`, as an unfinished rebase does; the
        // non-zero exit is the point, so it is not asserted.
        let _ = self
            .git_command(&midrebase)
            .args(["rebase", "-q", "--exec", "false", "HEAD~1"])
            .output()
            .expect("spawn git");
        assert!(
            midrebase.join(".git/rebase-merge").is_dir(),
            "midrebase is not mid-rebase"
        );
    }

    /// [`PREFIX_REPO`], [`WT_MAIN`] and [`WT_LINKED`] under `repo_dir`.
    /// `alpha` has one uncommitted edit. `wtmain` is a clone of its own
    /// remote, clean, with one stash and branches `side` and `feature` each
    /// one commit past their upstreams; `feature` is checked out in the
    /// linked worktree `wtmain-wt`, where its unpushed commit was made.
    pub fn add_worktree_and_prefix(&self) {
        let prefix = self.repo_dir.join(PREFIX_REPO);
        self.init(&prefix);
        self.commit(&prefix, "a.txt", "Start alpha", "2026-01-09T09:00:00Z");
        std::fs::write(prefix.join("a.txt"), "Start alpha, edited\n").unwrap();

        let main = self.repo_dir.join(WT_MAIN);
        let linked = self.repo_dir.join(WT_LINKED);
        self.publish(LOGIN, WT_MAIN, "2026-01-10T09:00:00Z");
        self.clone_into(LOGIN, WT_MAIN, &main);
        std::fs::remove_dir_all(self.root.join("seed")).unwrap();

        self.git(&main, &["switch", "-q", "-c", "side"]);
        self.commit(&main, "side.txt", "Side pushed", "2026-01-11T09:00:00Z");
        self.git(&main, &["push", "-q", "-u", "origin", "side"]);
        self.commit(&main, "side.txt", "Side unpushed", "2026-01-12T09:00:00Z");
        self.git(&main, &["switch", "-q", "main"]);

        self.git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                linked.to_str().unwrap(),
            ],
        );
        self.git(&linked, &["push", "-q", "-u", "origin", "feature"]);
        self.commit(
            &linked,
            "feature.txt",
            "Feature unpushed",
            "2026-01-13T09:00:00Z",
        );

        std::fs::write(main.join("README.md"), "Put aside\n").unwrap();
        self.git(&main, &["stash", "-q"]);
    }

    /// Tags for `-r`. `alpha-tool`'s HEAD is tagged `v1.0.0` (`released`);
    /// `orgproj` is tagged `v0.1.0` and then fast-forwarded one commit past
    /// it (`1 since`), still synced; `elsewhere-app`'s only tag is `latest`,
    /// which the `*[0-9]*` match skips (`unreleased`).
    pub fn add_releases(&self) {
        self.git(&self.user_clone, &["tag", "v1.0.0"]);
        self.git(&self.org_clone, &["tag", "v0.1.0"]);
        self.push_upstream(ORG, "orgproj", "Org release follow-up");
        self.git(&self.org_clone, &["pull", "-q", "--ff-only"]);
        self.git(&self.elsewhere_clone, &["tag", "latest"]);
    }

    /// Commits by [`OTHER_AUTHOR`] for `--author`: one pushed to
    /// `alpha-tool`'s remote and fetched, so the clone is two behind with
    /// two authors, and one unpushed in `elsewhere-app`, two ahead.
    pub fn add_other_author(&self) {
        let scratch = self.seed("alpha-tool");
        self.git(
            &self.root,
            &[
                "clone",
                "-q",
                &github_url(LOGIN, "alpha-tool"),
                scratch.to_str().unwrap(),
            ],
        );
        self.commit_as_other(
            &scratch,
            "lexer.txt",
            "Fix the lexer",
            "2026-03-12T11:00:00Z",
        );
        self.git(&scratch, &["push", "-q", "origin", "main"]);
        std::fs::remove_dir_all(&scratch).unwrap();
        self.git(&self.user_clone, &["fetch", "-q"]);

        self.commit_as_other(
            &self.elsewhere_clone,
            "layout.txt",
            "Sketch the layout",
            "2026-03-07T09:00:00Z",
        );
    }

    /// Open pull requests for `-p`: the payload, copied under `root` with
    /// `orgproj` at 2 and `ghost-repo` at 3 (`alpha-tool` keeps its 1), and
    /// read from there by both tools.
    pub fn add_pull_requests(&mut self) {
        let mut payload: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&self.payload).unwrap()).unwrap();
        let open = |node: &mut serde_json::Value| {
            let count = match node["name"].as_str() {
                Some("orgproj") => 2,
                Some(NOT_CLONED) => 3,
                _ => return,
            };
            node["pullRequests"]["totalCount"] = count.into();
        };
        let viewer = &mut payload["data"]["viewer"];
        viewer["repositories"]["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .for_each(open);
        for org in viewer["organizations"]["nodes"].as_array_mut().unwrap() {
            org["repositories"]["nodes"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .for_each(open);
        }
        let path = self.root.join("graphql-prs.json");
        std::fs::write(&path, payload.to_string()).unwrap();
        self.payload = path;
    }

    /// Remotes that do not exist, for D4's failure path: the bare remotes
    /// of `alpha-tool` (one behind, as of its last fetch) and `orgproj`
    /// (otherwise synced) are deleted, so a fetch of either fails offline.
    /// Their `FETCH_HEAD`s go too, so fleetz reads them as never fetched and
    /// fetches them (`stale_remotes`), as bash fetches every matched clone.
    pub fn add_missing_remotes(&self) {
        let clones = [
            (LOGIN, "alpha-tool", &self.user_clone),
            (ORG, "orgproj", &self.org_clone),
        ];
        for (owner, name, clone) in clones {
            let bare = self.remotes.join(owner).join(format!("{name}.git"));
            std::fs::remove_dir_all(bare).unwrap();
            std::fs::remove_file(clone.join(".git/FETCH_HEAD")).unwrap();
        }
    }

    fn init(&self, dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        self.git(dir, &["init", "-q"]);
    }

    fn write_configs(&self) {
        std::fs::create_dir_all(&self.repo_dir).unwrap();
        std::fs::create_dir_all(&self.extra_dir).unwrap();
        let gitconfig = format!(
            "[user]\n\tname = Fixture Author\n\temail = author@example.com\n\
             [init]\n\tdefaultBranch = main\n\
             [url \"{}/\"]\n\tinsteadOf = https://github.com/\n",
            self.remotes.display()
        );
        std::fs::write(self.home.join(".gitconfig"), gitconfig).unwrap();

        let config = self.home.join(".config");
        std::fs::create_dir_all(config.join("repoz")).unwrap();
        std::fs::write(
            config.join("repoz/config"),
            format!("EXTRA_DIRS={}\n", self.extra_dir.display()),
        )
        .unwrap();
        std::fs::create_dir_all(config.join("fleetz")).unwrap();
        std::fs::write(
            config.join("fleetz/config.toml"),
            format!(
                "repo_dir = '{}'\nextra_dirs = ['{}']\n",
                self.repo_dir.display(),
                self.extra_dir.display()
            ),
        )
        .unwrap();
    }

    /// A bare remote for `owner/name` holding one commit made at `date`,
    /// pushed from a seed checkout under `root/seed` (removed by `build`).
    pub fn publish(&self, owner: &str, name: &str, date: &str) {
        let bare = self.remotes.join(owner).join(format!("{name}.git"));
        std::fs::create_dir_all(&bare).unwrap();
        self.git(&bare, &["init", "-q", "--bare"]);
        let seed = self.seed(name);
        std::fs::create_dir_all(&seed).unwrap();
        self.git(&seed, &["init", "-q"]);
        self.commit(&seed, "README.md", &format!("Start {name}"), date);
        self.git(
            &seed,
            &["remote", "add", "origin", &github_url(owner, name)],
        );
        self.git(&seed, &["push", "-q", "origin", "main"]);
    }

    fn seed(&self, name: &str) -> PathBuf {
        self.root.join("seed").join(name)
    }

    /// Clone `owner/name` through its GitHub URL, then fetch once so
    /// `FETCH_HEAD` exists: a clone alone leaves none, and fleetz reads a
    /// missing one as never fetched.
    pub fn clone_into(&self, owner: &str, name: &str, dest: &Path) {
        let parent = dest.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        let dest = dest.to_str().unwrap();
        self.git(parent, &["clone", "-q", &github_url(owner, name), dest]);
        self.git(Path::new(dest), &["fetch", "-q"]);
    }

    fn commit(&self, dir: &Path, file: &str, subject: &str, date: &str) {
        std::fs::write(dir.join(file), format!("{subject}\n")).unwrap();
        self.git(dir, &["add", file]);
        let status = self
            .git_command(dir)
            .args(["commit", "-q", "-m", subject])
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .status()
            .expect("spawn git");
        assert!(status.success(), "git commit in {dir:?} failed");
    }

    /// [`Fixture::commit`] by [`OTHER_AUTHOR`].
    fn commit_as_other(&self, dir: &Path, file: &str, subject: &str, date: &str) {
        std::fs::write(dir.join(file), format!("{subject}\n")).unwrap();
        self.git(dir, &["add", file]);
        let status = self
            .git_command(dir)
            .args(["commit", "-q", "-m", subject])
            .env("GIT_AUTHOR_NAME", OTHER_AUTHOR)
            .env("GIT_AUTHOR_EMAIL", "other@example.com")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .status()
            .expect("spawn git");
        assert!(status.success(), "git commit in {dir:?} failed");
    }

    pub fn git(&self, dir: &Path, args: &[&str]) {
        let status = self
            .git_command(dir)
            .args(args)
            .status()
            .expect("spawn git");
        assert!(status.success(), "git {args:?} in {dir:?} failed");
    }

    fn git_command(&self, dir: &Path) -> Command {
        let mut cmd = Command::new("git");
        cmd.env_clear().envs(self.env()).current_dir(dir);
        cmd
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn github_url(owner: &str, name: &str) -> String {
    format!("https://github.com/{owner}/{name}.git")
}

/// `output` without its version line (`repoz 0.1.0`, flush right, the last
/// one of its kind), byte for byte otherwise. None when there is no version
/// line, so a comparison cannot pass on two outputs that both lost it.
pub fn without_version_line(output: &[u8]) -> Option<Vec<u8>> {
    let lines: Vec<&[u8]> = output.split_inclusive(|&b| b == b'\n').collect();
    let at = lines.iter().rposition(|line| is_version_line(line))?;
    Some(
        lines
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != at)
            .flat_map(|(_, line)| line.iter().copied())
            .collect(),
    )
}

/// A line reading `repoz <version>` (or `fleetz <version>`) once its escapes
/// and padding are off. The header also starts with `repoz`, but its second
/// word is never a version.
fn is_version_line(line: &[u8]) -> bool {
    let text = strip_ansi(&String::from_utf8_lossy(line));
    let words: Vec<&str> = text.split_whitespace().collect();
    matches!(words.as_slice(), [tool, version]
        if (*tool == "repoz" || *tool == "fleetz")
            && version.starts_with(|c: char| c.is_ascii_digit()))
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                break;
            }
        }
    }
    out
}

/// The recording for `case`, raw.
pub fn recorded(case: &str) -> Option<Vec<u8>> {
    std::fs::read(recording_path(case)).ok()
}

fn recording_path(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/report/fixtures")
        .join(format!("{case}.bash.out"))
}

/// Bash repoz to compare against: `REPOZ_BIN` when set (a patched copy, say),
/// else the sibling clone, so the repoz under development is the one checked,
/// else whatever `repoz` is on `PATH`. A `REPOZ_BIN` naming no file is no
/// repoz at all, so it skips as "not found" rather than as hookless.
fn repoz_bin() -> Option<PathBuf> {
    if let Some(bin) = std::env::var_os("REPOZ_BIN") {
        let bin = PathBuf::from(bin);
        return bin.is_file().then_some(bin);
    }
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

/// Bash repoz with the P1 fixture hook, or None after saying why not. Without
/// the hook it would call the real GitHub, so it is not run at all.
fn bash_with_hook() -> Option<PathBuf> {
    let Some(bin) = repoz_bin() else {
        eprintln!("SKIP: repoz not found, print parity recording unchecked");
        return None;
    };
    // A substring check, deliberately loose: any script that names the
    // variable counts as hooked, so a mention in a comment alone would pass.
    // The recording comparison below is what catches a hook that does not
    // work.
    let script = std::fs::read_to_string(&bin).unwrap_or_default();
    if !script.contains("REPOZ_GRAPHQL_FIXTURE") {
        eprintln!(
            "SKIP: {} has no REPOZ_GRAPHQL_FIXTURE hook (P1), print parity recording unchecked",
            bin.display()
        );
        return None;
    }
    Some(bin)
}

/// The child half of [`Fixture::run_print_mode`]: a no-op unless that
/// function started this process.
#[test]
fn print_mode_child() {
    let Some(out_path) = std::env::var_os(CHILD_OUT) else {
        return;
    };
    let args: Vec<String> = std::env::var(CHILD_ARGS)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let mut out = Vec::new();
    super::run(&args, &mut out).expect("print mode failed");
    std::fs::write(out_path, out).unwrap();
}

#[cfg(test)]
mod tests;
