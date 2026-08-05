# fleetz Async Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make fleetz open instantly, keep the UI responsive during all git and GitHub work, and turn watch mode into a real live dashboard (full per-repo refreshes, capped fetches, GitHub re-polling, blinking pending-CI indicator).

**Architecture:** Replace the blocking poll-and-draw loop with one tokio mpsc channel fed by an input thread, a 250ms tick task, and background worker tasks (load, sweep, GitHub poll). All git subprocess calls go through one hardened `run_git` wrapper with a timeout. Watch sweeps rebuild full repo status in parallel behind a semaphore, skipping expensive calls when a WorkKey (HEAD sha + index metadata) is unchanged.

**Tech Stack:** Rust 2024, tokio (already `features = ["full"]`: mpsc, JoinSet, Semaphore, time), ratatui 0.30, crossterm 0.29, reqwest, chrono. No new dependencies: use `std::thread::available_parallelism` instead of num_cpus.

## Global Constraints

- No new crates in Cargo.toml. tokio already ships everything needed.
- No em-dashes or en-dashes in any new or edited text: code, comments, README, this repo's docs. Use hyphens, colons, or parentheses. Do not mass-rewrite existing lines, but never introduce new dashes.
- `ui/` modules stay pure render-only: they read `&App` (plus plain params) and never mutate state or spawn work.
- Every task must pass: `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
- README.md must stay accurate: each task that changes flags, keys, config, or behavior updates the matching README section in the same task.
- The repo owner commits themselves: each task ends by staging and presenting a commit command, not running it, unless the owner has explicitly authorized committing.
- Out of scope for this plan (do not start them): notify-based filesystem watching (follow-up plan), splitting main.rs into modules beyond what the event loop forces, worktree overlay features.

---

### Task 1: Hardened git wrapper with timeout, plus first real-repo test

**Files:**
- Modify: `src/git.rs` (all subprocess call sites; wrapper goes at top after imports)
- Modify: `README.md` (Roadmap: remove the line about fetch hangs if present; no other section changes)

**Interfaces:**
- Produces: `fn git_cmd(repo_path: &Path, args: &[&str]) -> tokio::process::Command` (private)
- Produces: `pub(crate) async fn run_git(repo_path: &Path, args: &[&str]) -> Option<std::process::Output>` (returns None on spawn failure or 30s timeout; callers still check `output.status.success()`)
- Later tasks call `run_git` for new git helpers (Tasks 2 and 7).

- [x] **Step 1: Write the wrapper**

At the top of `src/git.rs` (after the `use` lines), add:

```rust
/// Hard ceiling per git invocation so one repo on a dead network mount or a
/// credential prompt can never wedge a sweep. kill_on_drop reaps the child.
const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Build a hardened git command: no optional locks (polling must not take
/// index.lock and fight editors), no terminal prompts (a fetch that wants
/// credentials fails instead of hanging), C locale (parsers match English
/// words like "insertion"), no color, stdin closed.
fn git_cmd(repo_path: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.arg("--no-optional-locks")
        .args(["-c", "color.ui=false"])
        .args(["-C", &repo_path.to_string_lossy()])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    c
}

/// Run a git command with the hardening and timeout. None means the process
/// could not run or timed out; callers treat that like a failed status.
pub(crate) async fn run_git(repo_path: &Path, args: &[&str]) -> Option<std::process::Output> {
    match tokio::time::timeout(GIT_TIMEOUT, git_cmd(repo_path, args).output()).await {
        Ok(Ok(output)) => Some(output),
        _ => None,
    }
}
```

- [x] **Step 2: Convert `fetch`, `branch_statuses`, `dirty_count` to the wrapper**

```rust
/// Run `git fetch` on a repo. Prunes deleted remote branches so
/// remote_only_branches does not accumulate stale entries.
pub async fn fetch(repo_path: &Path) -> bool {
    run_git(repo_path, &["fetch", "--quiet", "--prune"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}
```

In `branch_statuses` (git.rs:19) replace the `Command::new("git")...output().await` block with:

```rust
    let Some(output) = run_git(
        repo_path,
        &[
            "for-each-ref",
            "--format=%(refname:short)\t%(upstream:short)\t%(upstream:track)",
            "refs/heads/",
        ],
    )
    .await
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
```

Same shape for `dirty_count` (git.rs:122): `run_git(repo_path, &["status", "--porcelain"])`, keep the line-count logic.

- [x] **Step 3: Convert every remaining call site**

Each keeps its current parse and error behavior; only the invocation changes to `run_git(repo_path, &[...])` with these exact argument arrays:

| Function | New args array |
|---|---|
| `count_unreachable_from_remotes` | `["rev-list", "--count", branch, "--not", "--remotes"]` |
| `branch_commits` | unchanged args minus `-C path` pair (wrapper adds it) |
| `commits` | same rule: keep the existing `--date=relative`/format/numstat args, drop the leading `-C` pair |
| `reflog` | `["reflog", "--format=REFLOG|%h|%gs", "-n", &n.to_string()]` |
| `graph` | keep existing args minus the `-C` pair |
| `current_branch` | `["rev-parse", "--abbrev-ref", "HEAD"]` |
| `merged_branches` | keep existing args minus the `-C` pair |
| `all_branches` | `["branch", "--format=%(refname:short)"]` |
| `remote_branches` | `["for-each-ref", "--format=%(refname:short)", "refs/remotes/"]` |
| `remote_url` | `["remote", "get-url", "origin"]` |
| `switch_branch` | `["switch", &local]` |
| `delete_branch` | `["branch", "-d", name]` |
| `stash_list` | `["stash", "list", "--format=%s"]` |
| `worktree_list` | `["worktree", "list", "--porcelain"]` |
| `is_ff_possible`, `pull_ff_only` | delete both functions: dead code (never called), and this plan does not add pull |

Where a function built the args as a `Vec<String>` (commits with optional `--author`), collect to `Vec<String>` first and pass `&args.iter().map(String::as_str).collect::<Vec<_>>()`.

- [x] **Step 4: Write the real-repo integration test**

At the bottom of `src/git.rs`, add a second test module:

```rust
#[cfg(test)]
mod real_repo_tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command as StdCommand;

    fn sh(dir: &Path, args: &[&str]) {
        let status = StdCommand::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .status()
            .expect("spawn");
        assert!(status.success(), "command failed: {args:?}");
    }

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fleetz_it_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["git", "init", "-q", "-b", "main"]);
        sh(&dir, &["git", "config", "user.email", "t@example.com"]);
        sh(&dir, &["git", "config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh(&dir, &["git", "commit", "-q", "-m", "init"]);
        dir
    }

    #[tokio::test]
    async fn branch_statuses_and_dirty_count_on_real_repo() {
        let dir = temp_repo("basic");

        let branches = branch_statuses(&dir).await;
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "main");
        assert_eq!(branches[0].upstream, None);

        assert_eq!(dirty_count(&dir).await, 0);
        std::fs::write(dir.join("b.txt"), "two\n").unwrap();
        assert_eq!(dirty_count(&dir).await, 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [x] **Step 5: Run the gates**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all existing parser tests still pass (LC_ALL=C changes nothing for them; they feed strings), new test passes.

- [x] **Step 6: Stage and present the commit**

```bash
git add src/git.rs README.md
git commit -m "refactor: route all git calls through a hardened run_git wrapper

--no-optional-locks, prompt-free env, C locale, 30s timeout, kill_on_drop.
Adds the first real-repo integration test. Drops dead pull helpers."
```

---

### Task 2: Correctness fixes (cache key, offline fallback, dead flag, merged base, worktree discovery, opener)

**Files:**
- Modify: `src/cache.rs:12-20`, `src/github.rs:165-228`, `src/cli.rs:33-35,108-113`, `src/app.rs:395,450-486`, `src/main.rs:74,502,713-721`, `src/discovery.rs:109-113`, `src/git.rs` (new `default_branch`), `README.md` (flags table, keybindings note)

**Interfaces:**
- Consumes: `run_git` from Task 1.
- Produces: `ApiCache::new(ttl_secs: u64, repo_limit: u32, include_prs: bool) -> Self`
- Produces: `pub async fn default_branch(repo_path: &Path) -> Option<String>` in git.rs
- Produces: `App::new(repos, header, show_prs, term_width, term_height)` loses the `show_stat` param; `App.show_stat` field is gone. Later tasks use the 5-param `App::new`.

- [x] **Step 1: Write the failing cache-key test** (in `src/cache.rs` tests)

```rust
    #[test]
    fn cache_path_varies_by_query_shape() {
        let a = ApiCache::new(300, 50, false);
        let b = ApiCache::new(300, 50, true);
        let c = ApiCache::new(300, 100, false);
        assert_ne!(a.path, b.path);
        assert_ne!(a.path, c.path);
    }
```

Make `path` visible to the test (it is in the same module; keep the field private, the test can read it). Run `cargo test cache_path_varies` and confirm it fails to compile against the old one-arg `new`.

- [x] **Step 2: Implement the keyed cache**

```rust
    pub fn new(ttl_secs: u64, repo_limit: u32, include_prs: bool) -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("fleetz");
        // Pre-keying versions wrote a single github.json for every query
        // shape; remove it so it cannot be served for the wrong flags.
        let _ = std::fs::remove_file(cache_dir.join("github.json"));
        let prs = if include_prs { "prs" } else { "noprs" };
        Self {
            path: cache_dir.join(format!("github-{repo_limit}-{prs}.json")),
            ttl: Duration::from_secs(ttl_secs),
        }
    }
```

Update the call site `main.rs:925` to `cache::ApiCache::new(config.cache_ttl, config.repo_limit, args.prs)`. Update the two existing cache tests to construct via struct literal as they already do (unchanged) plus the new test.

- [x] **Step 3: Offline fallback on transport error** (`src/github.rs`)

Extract the repeated stale-parse into a helper above `fetch_repos`:

```rust
/// Parse a cached raw response body into repos, if it parses at all.
fn parse_cached(json: Option<String>) -> Option<Vec<GithubRepo>> {
    let response: GraphQlResponse = serde_json::from_str(&json?).ok()?;
    Some(parse_repos(response.data?.viewer.repositories.nodes))
}
```

Replace the `send().await.map_err(...)` at github.rs:189-196 with:

```rust
    let resp = match client
        .post("https://api.github.com/graphql")
        .header("Authorization", format!("bearer {token}"))
        .header("User-Agent", "fleetz")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
    {
        Ok(r) => r,
        // Transport failure (offline, DNS, TLS): serve stale cache if any,
        // matching the non-2xx path below, so launch works offline.
        Err(e) => {
            if let Some(repos) = parse_cached(cache.read_stale()) {
                return Ok(repos);
            }
            return Err(format!("GitHub API request failed: {e}"));
        }
    };
```

Also rewrite the two existing inline stale-parse blocks (cache hit at :175-181 and non-2xx at :199-206) to use `parse_cached(cache.read())` / `parse_cached(cache.read_stale())`.

- [x] **Step 4: Remove the dead `-s/--stat` flag**

It is parsed and stored but never read (App.show_stat has zero readers; commits are always fetched with stats). Wiring it up instead was considered and rejected: it would turn stats off by default, a silent behavior change. Remove: the `stat` field and its arg from `src/cli.rs:33-35`; `show_stat` from `App` and `App::new` (app.rs:395, 450-486); the fourth argument at main.rs:74; the `-s` assertions in cli.rs `combined_flags` test; the `-s, --stat` row in README's flags table and the `-s` example line.

- [x] **Step 5: Merged-branch base = default branch** (`src/git.rs`, `src/main.rs:500-502`)

Add to git.rs:

```rust
/// The repo's default branch: origin/HEAD if known, else main/master if they
/// exist locally. None when neither is resolvable (offline bare-ish repos).
pub async fn default_branch(repo_path: &Path) -> Option<String> {
    if let Some(o) = run_git(repo_path, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).await {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if let Some(name) = s.strip_prefix("origin/") {
                return Some(name.to_string());
            }
        }
    }
    for cand in ["main", "master"] {
        if let Some(o) = run_git(
            repo_path,
            &["show-ref", "--verify", "--quiet", &format!("refs/heads/{cand}")],
        )
        .await
        {
            if o.status.success() {
                return Some(cand.to_string());
            }
        }
    }
    None
}
```

In `open_branch_overlay` (main.rs:500-502):

```rust
    let current = git::current_branch(repo_path).await.unwrap_or_default();
    let base = git::default_branch(repo_path).await.unwrap_or_else(|| current.clone());
    let all = git::all_branches(repo_path).await;
    let merged = git::merged_branches(repo_path, &base).await;
```

Rename `merged_branches`'s parameter from `default_branch` to `base_branch` so the name stops lying. Add a real-repo test in `real_repo_tests`: create a repo on main, branch `feature`, merge it, sit on a third branch, assert `default_branch` returns `main` (via the main/master fallback) and `merged_branches(path, "main")` contains `feature`.

- [x] **Step 6: Discover linked worktrees** (`src/discovery.rs:109-113`)

```rust
    let git_dir = dir.join(".git");
    // A .git *file* (gitdir pointer) marks a linked worktree or submodule
    // checkout; both are real working trees worth listing.
    if git_dir.is_dir() || git_dir.is_file() {
        callback(dir);
        return; // don't descend into git repos
    }
```

Add a test: create `wt/.git` as a plain file, assert `find_all_local` returns it.

- [x] **Step 7: Platform-aware browser opener** (`src/main.rs`)

Near the constants at main.rs:34-40:

```rust
/// Browser-opener binary: macOS ships `open`, everything else gets xdg-open.
#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const OPENER: &str = "xdg-open";
```

At main.rs:717 replace `StdCommand::new("open")` with `StdCommand::new(OPENER)`.

- [x] **Step 8: Run the gates, stage, present the commit**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`

```bash
git add src/cache.rs src/github.rs src/cli.rs src/app.rs src/main.rs src/discovery.rs src/git.rs README.md
git commit -m "fix: cache key per query shape, offline stale fallback, merged-base, worktree discovery

Also removes the dead -s/--stat flag and hardcoded macOS opener."
```

---

### Task 3: Channel-based event loop (input thread, tick task, dirty-flag redraw)

**Files:**
- Modify: `src/main.rs` (replace `run_event_loop` main.rs:233-336; add event plumbing; adjust `main`)
- Modify: `src/app.rs` (new fields)

**Interfaces:**
- Produces (main.rs):

```rust
pub enum AppEvent {
    Input(crossterm::event::Event),
    Tick,
    Loaded(Result<Box<LoadResult>, String>),
    RepoUpdated { path: String, status: Box<RepoStatus> },
    Github(Result<Vec<github::GithubRepo>, String>),
    Progress(app::ApplyProgress),
    SweepDone { at: chrono::DateTime<chrono::Local> },
}

pub enum LoadKind { Initial, Reload, WindowApply(time::TimeWindow) }

pub struct LoadResult {
    pub kind: LoadKind,
    pub repos: Vec<RepoStatus>,
    pub header: String,
    /// None for WindowApply (dataset unchanged by a pure re-filter).
    pub dataset: Option<Vec<github::GithubRepo>>,
}
```

- Produces (app.rs additions): `needs_redraw: bool`, `spinner_frame: usize`, `blink_on: bool`, `view_overridden: bool`, `loading: bool`, plus `pub fn on_tick(&mut self)` (increments spinner_frame; flips blink_on every second call).
- Produces: `load_repos` and everything under it switch their error type from `Box<dyn Error>` to `String` so results can cross task boundaries (`assemble_view`, `load_repos` signatures change; `apply_time_window`'s stringify workaround disappears).
- `RepoUpdated`/`Github`/`SweepDone` are emitted by Tasks 5-6; this task only defines them and their loop arms.
- Consumes: 5-param `App::new` from Task 2.

- [x] **Step 1: Add the app fields**

In `App` (app.rs:384) add, and initialize in `App::new`:

```rust
    /// Repaint on the next loop pass. Set by every state change; ticks also
    /// force a paint every 4th frame so the clock and spinner stay live.
    pub needs_redraw: bool,        // init: true
    /// Advances every 250ms tick; drives the loading/sweep spinner.
    pub spinner_frame: usize,      // init: 0
    /// Flips every 500ms; drives the pending-CI blink (Task 8).
    pub blink_on: bool,            // init: true
    /// True once the user pressed v; stops resize from auto-switching views.
    pub view_overridden: bool,     // init: false
    /// True while the initial load or an R reload is in flight.
    pub loading: bool,             // init: false
```

```rust
    /// Called on every 250ms tick from the event loop.
    pub fn on_tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
        if self.spinner_frame % 2 == 0 {
            self.blink_on = !self.blink_on;
        }
    }
```

- [x] **Step 2: Spawn input thread and tick task**

In main.rs:

```rust
fn spawn_input_thread(tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
    // Dedicated OS thread: crossterm's blocking read never parks the runtime.
    std::thread::spawn(move || loop {
        match event::read() {
            Ok(ev) => {
                if tx.send(AppEvent::Input(ev)).is_err() {
                    break; // loop ended, channel closed
                }
            }
            Err(_) => break,
        }
    });
}

fn spawn_tick_task(tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            interval.tick().await;
            if tx.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });
}
```

- [x] **Step 3: Rewrite `run_event_loop`**

```rust
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Result<(), String> {
    terminal.draw(|frame| ui::render(frame, app)).map_err(|e| e.to_string())?;

    while let Some(ev) = rx.recv().await {
        match ev {
            AppEvent::Tick => {
                app.on_tick();
                if app.status_expired(STATUS_TTL) {
                    app.clear_status();
                    app.needs_redraw = true;
                }
                // Keep spinner/blink/clock moving: paint every 4th tick while
                // anything animated is on screen, else stay idle.
                let animated = app.loading || app.watch_mode || app.apply_progress.is_some();
                if animated && app.spinner_frame % 4 == 0 {
                    app.needs_redraw = true;
                }
                // Task 6 adds the watch due-check here.
            }
            AppEvent::Input(Event::Key(key)) => {
                app.clear_status();
                handle_key(app, terminal, key, &tx, args, config, repo_dir).await;
                app.needs_redraw = true;
            }
            AppEvent::Input(Event::Mouse(m)) => {
                handle_mouse_input(app, terminal, m);
                app.needs_redraw = true;
            }
            AppEvent::Input(Event::Resize(w, h)) => {
                if !app.view_overridden {
                    app.view = app::initial_view(w, h);
                }
                app.needs_redraw = true;
            }
            AppEvent::Input(_) => {}
            AppEvent::Loaded(result) => {
                apply_load_result(app, config, result);
                app.needs_redraw = true;
            }
            AppEvent::Progress(p) => {
                app.apply_progress = Some(p);
                app.needs_redraw = true;
            }
            AppEvent::RepoUpdated { path, status } => {
                if let Some(repo) = app
                    .repos
                    .iter_mut()
                    .find(|r| r.path.as_deref() == Some(path.as_str()))
                {
                    *repo = *status;
                }
                app.needs_redraw = true;
            }
            AppEvent::Github(result) => {
                // Handled fully in Task 6 (re-poll); until then only errors land.
                if let Err(e) = result {
                    app.set_status(format!("GitHub refresh failed: {e}"), true);
                }
                app.needs_redraw = true;
            }
            AppEvent::SweepDone { at } => {
                app.last_refreshed = Some(at);
                app.needs_redraw = true;
            }
        }

        if app.should_quit {
            return Ok(());
        }
        if app.needs_redraw {
            app.needs_redraw = false;
            terminal.draw(|frame| ui::render(frame, app)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
```

`handle_key` is the old overlay/search/normal dispatch (main.rs:288-320) moved into a function; `handle_normal_input` keeps its body except: `v` also sets `app.view_overridden = true`; the `R` arm and the time-window Enter no longer use deferred flags (Step 4). Delete the `poll_timeout` computation, the inline watch block (main.rs:265-276), and the `reload_requested` service block; delete the `reload_requested` and `window_apply_requested` fields from App.

- [x] **Step 4: Background load path**

Replace `reload_all` and `apply_time_window` with spawns; change `load_repos`/`assemble_view` error type to `String` and `assemble_view`'s progress param to `Option<&tokio::sync::mpsc::UnboundedSender<AppEvent>>` sending `AppEvent::Progress(...)`:

```rust
fn spawn_load(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    window: Option<time::TimeWindow>,
    kind: LoadKind,
    bypass_cache: bool,
) {
    let tx = tx.clone();
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    tokio::spawn(async move {
        let mode = args.mode();
        let result = match kind {
            LoadKind::WindowApply(w) => {
                // Pure re-filter of the dataset; caller passed it via window.
                unreachable!("window apply uses spawn_window_apply")
            }
            _ => load_repos(&args, &config, &repo_dir, &mode, window, bypass_cache)
                .await
                .map(|(repos, header, dataset)| {
                    Box::new(LoadResult { kind, repos, header, dataset: Some(dataset) })
                }),
        };
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

fn spawn_window_apply(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    app: &App,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    w: time::TimeWindow,
) {
    let tx = tx.clone();
    let dataset = app.dataset.clone();
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    tokio::spawn(async move {
        let cutoff = w.cutoff(config.work_start, config.work_end);
        let header = w.label(config.work_start, config.work_end);
        let result = assemble_view(&dataset, Some(cutoff), header, &args, &config, &repo_dir, false, Some(&tx))
            .await
            .map(|(repos, header)| {
                Box::new(LoadResult { kind: LoadKind::WindowApply(w), repos, header, dataset: None })
            });
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

fn apply_load_result(app: &mut App, config: &config::Config, result: Result<Box<LoadResult>, String>) {
    app.apply_progress = None;
    app.loading = false;
    match result {
        Ok(lr) => {
            app.repos = lr.repos;
            apply_groups(&mut app.repos, config);
            app.header = lr.header;
            if let Some(ds) = lr.dataset {
                app.dataset = ds;
            }
            app.clamp_after_reload();
            app.last_refreshed = Some(chrono::Local::now());
            match lr.kind {
                LoadKind::Initial => {}
                LoadKind::Reload => app.set_status("Reloaded".to_string(), false),
                LoadKind::WindowApply(w) => {
                    window_state::WindowState::new().save(&w);
                    app.set_status(format!("Window: {}", app.header), false);
                }
            }
        }
        Err(e) => app.set_status(format!("Load failed: {e}"), true),
    }
}
```

Key arms change to: `R` sets `app.loading = true`, status "Reloading", `spawn_load(&tx, args, config, repo_dir, app.window, LoadKind::Reload, true)`. Time-window Enter calls `spawn_window_apply(&tx, app, args, config, repo_dir, w)` and sets `app.apply_progress = Some(ApplyProgress::Discovering)`. Simplify `unreachable!` away by making spawn_load's match a plain call (the WindowApply variant is only ever constructed by spawn_window_apply; add a debug_assert instead of the match if cleaner).

- [x] **Step 5: Run gates and verify by hand**

Run: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings`
Manual: `cargo run --release` then press keys rapidly during an R reload; the UI must stay responsive (selection moves while "Reloading" shows). Resize the terminal below 70 columns: view flips to Compact; press v: it stays put on further resizes.

- [x] **Step 6: Stage and present the commit**

```bash
git add src/main.rs src/app.rs
git commit -m "refactor: channel-driven event loop with input thread and 250ms tick

One mpsc feeds input, ticks, and background results. Redraw is dirty-flag
driven, reload and window-apply run as tasks, resize re-picks the view."
```

---

### Task 4: Instant startup

**Files:**
- Modify: `src/main.rs:53-107` (`main`), `src/ui/mod.rs:306-402` (`render_header`)
- Modify: `README.md` (remove the Roadmap bullet about refresh freezing the UI; note instant open)

**Interfaces:**
- Consumes: `spawn_load`, `LoadKind::Initial`, `app.loading`, `app.spinner_frame` from Task 3.
- Produces: header shows a braille spinner while `app.loading`.

- [x] **Step 1: Reorder `main`**

Terminal first, data never blocks the first frame:

```rust
    let mode = args.mode();
    let initial_window = if mode == Mode::Auto && args.before.is_none() {
        window_state::WindowState::new().load()
    } else {
        None
    };

    let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut app = App::new(Vec::new(), String::new(), args.prs, term_width, term_height);
    app.watch_mode = args.watch;
    app.refresh_interval = config.refresh_interval;
    app.window = initial_window;
    app.dirty_mode = mode == Mode::Dirty;
    app.loading = true;
    if !config.groups.is_empty() {
        app.group_names = config.groups.keys().cloned().collect();
        app.group_names.sort();
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    spawn_input_thread(tx.clone());
    spawn_tick_task(tx.clone());
    spawn_load(&tx, &args, &config, &repo_dir, initial_window, LoadKind::Initial, false);

    let result = run_event_loop(&mut terminal, &mut app, &args, &config, &repo_dir, tx, rx).await;
```

The teardown block stays; map the `String` error into the outer `Box<dyn Error>` at the end (`result.map_err(|e| e.into())`).

- [x] **Step 2: Loading spinner in the header**

In `render_header` (ui/mod.rs:306), before `ts_span` is built, add:

```rust
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let loading_span = app.loading.then(|| {
        Span::styled(
            format!(" {} loading ", SPINNER[app.spinner_frame % SPINNER.len()]),
            Style::default().fg(colors::PEACH),
        )
    });
```

Push it first in the `right` vec (before the watch span), unconditionally included in every drop-out iteration (it is small and the most important signal while it lasts). While `app.loading` and `app.repos.is_empty()`, the repo list panel already renders empty; that is acceptable for this task.

- [x] **Step 3: Verify**

Run: `cargo build --release && time cargo run --release` (with a cold cache: `rm ~/Library/Caches/fleetz/github-*.json`).
Expected: the TUI frame with the spinner appears in well under a second; repos pop in when the load lands. `cargo test`, clippy, fmt gates pass.

- [x] **Step 4: Stage and present the commit**

```bash
git add src/main.rs src/ui/mod.rs README.md
git commit -m "feat: open the TUI instantly, load GitHub and discovery in the background

First frame renders before any network or filesystem work, with a header
spinner until the initial load lands."
```

---

### Task 5: Parallel probing with capped concurrency

**Files:**
- Modify: `src/main.rs` (`assemble_view` fetch fan-out :1054-1066 and both sequential stat loops :1073-1102; pass-3 loop :1113-1173; dirty-mode loop :862-920)

**Interfaces:**
- Produces: `fn probe_concurrency() -> usize` (available_parallelism clamped 2..=12), `const FETCH_CONCURRENCY: usize = 4;`
- Consumes: `run_git` timeout semantics from Task 1 (a hung fetch dies at 30s, so the join cannot wedge).

- [ ] **Step 1: Concurrency helpers**

```rust
/// Parallel git-status probes: enough to hide subprocess latency, capped so a
/// big fleet does not spawn hundreds of processes at once.
fn probe_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 12)
}

/// Network fetches are remote-bound, not CPU-bound: keep it polite.
const FETCH_CONCURRENCY: usize = 4;
```

- [ ] **Step 2: Cap the fetch fan-out** (replaces main.rs:1054-1066)

```rust
    if git_fetch {
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for f in &all_found {
            let path = f.path.clone();
            let sem = sem.clone();
            set.spawn(async move {
                let _permit = sem.acquire().await;
                git::fetch(&path).await
            });
        }
        while set.join_next().await.is_some() {}
    }
```

- [ ] **Step 3: Parallelize the stat loops**

Replace the two sequential loops (primary then elsewhere) with one JoinSet keyed by index so output order is stable:

```rust
    let mut targets: Vec<(discovery::FoundRepo, RepoLocation)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for f in &primary_found {
        if seen.insert(f.name.to_lowercase()) {
            targets.push((f.clone(), RepoLocation::Primary));
        }
    }
    for f in &elsewhere_found {
        if seen.insert(f.name.to_lowercase()) {
            targets.push((f.clone(), RepoLocation::Elsewhere));
        }
    }

    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
    let mut set = tokio::task::JoinSet::new();
    for (i, (found, location)) in targets.iter().cloned().enumerate() {
        let sem = sem.clone();
        let active = active.clone();
        let args = args.clone();
        set.spawn(async move {
            let _permit = sem.acquire().await;
            (i, build_repo_status(&found, &active, &args, location).await)
        });
    }

    let mut slots: Vec<Option<RepoStatus>> = vec![None; targets.len()];
    let mut stat_done = 0usize;
    while let Some(joined) = set.join_next().await {
        if let Ok((i, repo)) = joined {
            slots[i] = Some(repo);
            stat_done += 1;
            if let Some(tx) = progress {
                let _ = tx.send(AppEvent::Progress(app::ApplyProgress::Statting {
                    done: stat_done,
                    total: targets.len(),
                }));
            }
        }
    }
    let mut repos: Vec<RepoStatus> = slots.into_iter().flatten().collect();
    let mut checked_names: HashSet<String> = seen;
```

`checked_names` then feeds pass 3 unchanged. Note `stat_total` becomes `targets.len()` (dedup happens before counting, fixing the gauge-accounting workaround at main.rs:1086-1093). Parallelize pass 3's per-path `dirty_count`/`branch_statuses` filter with the same JoinSet pattern (spawn per path, collect `Option<RepoStatus>`); the dirty-mode loop in `load_repos` (main.rs:862-920) gets the identical treatment.

- [ ] **Step 4: Verify timing**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Manual: `time cargo run --release -- -a` before and after this task (note the wall time in the task notes; expect roughly a probe_concurrency-fold improvement on the stat phase).

- [ ] **Step 5: Stage and present the commit**

```bash
git add src/main.rs
git commit -m "perf: probe repos in parallel behind a semaphore, cap fetch fan-out at 4"
```

---

### Task 6: Real watch sweeps (full refresh, capped fetch, GitHub re-poll, visible cadence)

**Files:**
- Modify: `src/main.rs` (sweep machinery; delete `refresh_single_repo` :811-837; rework `f`/`F`/`r` arms; Tick arm), `src/app.rs` (sweep fields), `src/cli.rs` (add `--interval`), `src/config.rs:31` (default 60), `src/ui/mod.rs` (header "swept Xs ago"), `README.md` (config default, --interval flag, Roadmap: mark background refresh + fetch-in-cycle done)

**Interfaces:**
- Produces (main.rs):

```rust
struct SweepTarget {
    /// Display path as stored in RepoStatus.path (tilde form), the update key.
    display_path: String,
    fs_path: std::path::PathBuf,
    /// Existing status, carried so GitHub-only fields survive the rebuild.
    old: Box<RepoStatus>,
}

fn start_sweep(app: &mut App, tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>, do_fetch: bool, author: Option<String>);
async fn rebuild_repo_status(target: &SweepTarget, author: Option<&str>) -> RepoStatus;
const SWEEP_STUCK_AFTER: std::time::Duration = std::time::Duration::from_secs(180);
```

- Produces (app.rs): `sweep_started: Option<std::time::Instant>` (None = idle), `last_github_poll: Option<chrono::DateTime<chrono::Local>>`, `github_poll_inflight: bool`.
- Produces (cli.rs): `#[arg(long)] pub interval: Option<u64>` ("Watch refresh interval in seconds (default: config refresh_interval)").
- Consumes: `AppEvent::{RepoUpdated, SweepDone, Github}`, semaphores from Task 5.

- [ ] **Step 1: CLI flag and config default**

cli.rs: add the `interval` field as above. main.rs after config load: `app.refresh_interval = args.interval.unwrap_or(config.refresh_interval);`. config.rs:31: `refresh_interval: 60`. README: config snippet comment becomes `# Auto-refresh interval in seconds (used with --watch, default 60)`; add `--interval SECS` to the options table.

- [ ] **Step 2: The sweep**

```rust
/// Kick off a background sweep of every repo with a local path. Each repo is
/// fetched (optionally), rebuilt in full, and streamed back as RepoUpdated;
/// SweepDone closes the books. No-op if a sweep is already running.
fn start_sweep(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    do_fetch: bool,
    author: Option<String>,
) {
    if app.sweep_started.is_some() {
        return;
    }
    let targets: Vec<SweepTarget> = app
        .repos
        .iter()
        .filter_map(|r| {
            let display = r.path.clone()?;
            let expanded = shellexpand::tilde(&display).to_string();
            Some(SweepTarget {
                display_path: display,
                fs_path: std::path::PathBuf::from(expanded),
                old: Box::new(r.clone()),
            })
        })
        .collect();
    if targets.is_empty() {
        return;
    }
    app.sweep_started = Some(std::time::Instant::now());

    let tx = tx.clone();
    tokio::spawn(async move {
        let probe_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
        let fetch_sem = std::sync::Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for target in targets {
            let probe_sem = probe_sem.clone();
            let fetch_sem = fetch_sem.clone();
            let tx = tx.clone();
            let author = author.clone();
            set.spawn(async move {
                if do_fetch {
                    let _permit = fetch_sem.acquire().await;
                    git::fetch(&target.fs_path).await;
                }
                let _permit = probe_sem.acquire().await;
                let status = rebuild_repo_status(&target, author.as_deref()).await;
                let _ = tx.send(AppEvent::RepoUpdated {
                    path: target.display_path.clone(),
                    status: Box::new(status),
                });
            });
        }
        while set.join_next().await.is_some() {}
        let _ = tx.send(AppEvent::SweepDone { at: chrono::Local::now() });
    });
}

/// Full per-repo rebuild, preserving GitHub-only fields from the old status.
/// (Task 7 makes the heavy half conditional on a WorkKey.)
async fn rebuild_repo_status(target: &SweepTarget, author: Option<&str>) -> RepoStatus {
    let path = &target.fs_path;
    let old = &target.old;

    let branches = git::branch_statuses(path).await;
    let dirty = git::dirty_count(path).await;
    let remote = git::remote_branches(path).await;
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let (total_behind, total_ahead) = branches
        .iter()
        .fold((0u32, 0u32), |(b, a), s| (b + s.behind, a + s.ahead));

    let commits = git::commits(path, &format!("-{RECENT_COMMIT_COUNT}"), true, author).await;
    let reflog = git::reflog(path, REFLOG_COUNT).await;
    let graph = git::graph(path, &branch_names, GRAPH_COMMIT_COUNT).await;
    let stash_entries = git::stash_list(path).await;
    let worktrees = git::worktree_list(path).await;

    RepoStatus {
        branches,
        dirty,
        behind: total_behind,
        ahead: total_ahead,
        remote_only_branches: git::unfetched_branches(&remote, &branch_names),
        stash_count: stash_entries.len() as u32,
        stash_entries,
        commits,
        reflog,
        graph,
        worktrees,
        ..(*old.clone())
    }
}
```

Delete `refresh_single_repo` (main.rs:811-837). Rework the callers: `r` sets status "Refreshing" and calls `start_sweep(app, &tx, false, args.author.clone())`; `F` sets status "Fetching all" and calls `start_sweep(app, &tx, true, args.author.clone())` (the old sequential loop at main.rs:635-655 goes away); `f` and the branch-switch refresh (main.rs:429, 626) build a single-target `SweepTarget` for the selected repo and spawn the same task body via a `start_single_sweep(app, tx, index, do_fetch)` helper that shares the spawn code with `start_sweep`.

- [ ] **Step 3: Tick-driven watch cadence and GitHub re-poll**

In the `AppEvent::Tick` arm from Task 3, after `app.on_tick()`:

```rust
    // A sweep that never reported back must not block watch forever.
    if let Some(started) = app.sweep_started {
        if started.elapsed() > SWEEP_STUCK_AFTER {
            app.sweep_started = None;
        }
    }
    if app.watch_mode && !app.loading && app.sweep_started.is_none() {
        let due = app
            .last_refreshed
            .map_or(true, |t| (chrono::Local::now() - t).num_seconds() >= app.refresh_interval as i64);
        if due {
            start_sweep(app, &tx, true, args.author.clone());
        }
    }
    if app.watch_mode && !app.dirty_mode && !app.loading && !app.github_poll_inflight {
        let poll_every = config.cache_ttl.max(120) as i64;
        let due = app
            .last_github_poll
            .map_or(true, |t| (chrono::Local::now() - t).num_seconds() >= poll_every);
        if due {
            app.github_poll_inflight = true;
            spawn_github_poll(&tx, args, config);
        }
    }
```

Note the `map_or(true, ...)`: a `None` timestamp means "due now", so watch can never silently die the way the old `if let Some(last)` guard could. `SweepDone` sets `app.sweep_started = None` in addition to `last_refreshed`.

```rust
fn spawn_github_poll(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
) {
    let tx = tx.clone();
    let repo_limit = config.repo_limit;
    let include_prs = args.prs;
    let cache = cache::ApiCache::new(config.cache_ttl, repo_limit, include_prs);
    tokio::spawn(async move {
        let result = github::fetch_repos(repo_limit, include_prs, &cache, true).await;
        let _ = tx.send(AppEvent::Github(result));
    });
}
```

The `AppEvent::Github(Ok(dataset))` arm now does real work:

```rust
            AppEvent::Github(result) => {
                app.github_poll_inflight = false;
                app.last_github_poll = Some(chrono::Local::now());
                match result {
                    Ok(dataset) => {
                        for repo in &mut app.repos {
                            if let Some(gh) = dataset
                                .iter()
                                .find(|g| g.name.eq_ignore_ascii_case(&repo.name))
                            {
                                repo.pr_count = gh.pr_count;
                                repo.prs = gh.prs.clone();
                                repo.ci_status = gh.ci_status.clone();
                                repo.ci_workflow = gh.ci_workflow.clone();
                                repo.ci_duration = gh.ci_duration.clone();
                                repo.pushed_at = Some(gh.pushed_at);
                                repo.repo_kind = gh.repo_kind;
                            }
                        }
                        app.dataset = dataset;
                    }
                    Err(e) => app.set_status(format!("GitHub refresh failed: {e}"), true),
                }
                app.needs_redraw = true;
            }
```

- [ ] **Step 4: Visible cadence in the header**

In `render_header`, replace the `ts_span` "refreshed HH:MM" builder (ui/mod.rs:329-334) with an age label, and show a spinner while sweeping:

```rust
    let ts_span = if app.sweep_started.is_some() {
        Some(Span::styled(
            format!(" {} sweeping ", SPINNER[app.spinner_frame % SPINNER.len()]),
            Style::default().fg(colors::PEACH),
        ))
    } else {
        app.last_refreshed.map(|ts| {
            let secs = (chrono::Local::now() - ts).num_seconds().max(0);
            let label = if secs < 5 {
                "just now".to_string()
            } else if secs < 90 {
                format!("{secs}s ago")
            } else {
                format!("{}m ago", secs / 60)
            };
            Span::styled(format!(" swept {label}  "), Style::default().fg(colors::SURFACE))
        })
    };
```

(The `SPINNER` const from Task 4 moves to module scope in ui/mod.rs so both spans share it.)

- [ ] **Step 5: Verify by hand**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`, then:
- `cargo run --release -- --watch --interval 10`. Touch a file in a listed repo (`touch ~/repos/<name>/x.tmp`): the repo's `!` count changes within ~10s with no keypress, and "swept Ns ago" resets. Remove the file.
- Commit in a repo: the commit appears in the detail pane's recent list after the next sweep (this is the field the old watch never updated).
- Watch a repo with a running GitHub Action: the CI badge updates within `max(cache_ttl, 120)` seconds.
- UI stays fully responsive during sweeps (scroll while the spinner runs).

- [ ] **Step 6: Stage and present the commit**

```bash
git add src/main.rs src/app.rs src/cli.rs src/config.rs src/ui/mod.rs README.md
git commit -m "feat: real watch sweeps with fetch, GitHub re-poll, and visible cadence

Watch now rebuilds full repo status in parallel, includes capped fetches,
re-polls GitHub for PR/CI, and shows swept-ago in the header. Default
interval drops to 60s and --interval overrides it."
```

---

### Task 7: WorkKey probe skip (cheap sweeps when nothing changed)

**Files:**
- Modify: `src/git.rs` (WorkKey + resolve helpers + tests), `src/model.rs` (field), `src/main.rs` (`build_repo_status`, `rebuild_repo_status`)

**Interfaces:**
- Produces (git.rs):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkKey {
    /// Output of rev-parse HEAD: one cheap subprocess buys exact commit identity.
    pub head_sha: String,
    pub index_mtime: Option<std::time::SystemTime>,
    pub index_size: u64,
    /// Size of the stash reflog (common dir aware); 0 when absent.
    pub stash_size: u64,
}

pub fn resolve_git_dir(repo_path: &Path) -> Option<std::path::PathBuf>;
pub async fn work_key(repo_path: &Path) -> Option<WorkKey>;
```

- Produces (model.rs): `pub work_key: Option<crate::git::WorkKey>` on RepoStatus (init `None` at every current literal construction site: main.rs dirty-mode, pass-3, not-cloned, build_repo_status).
- Consumes: `run_git` (Task 1), `rebuild_repo_status` (Task 6), temp-repo harness (Task 1).

- [ ] **Step 1: Implement the key**

```rust
/// The repo's real git directory: `.git` itself, or the target of a
/// `gitdir:` pointer file (linked worktrees, submodule checkouts).
pub fn resolve_git_dir(repo_path: &Path) -> Option<std::path::PathBuf> {
    let dot = repo_path.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.strip_prefix("gitdir:")?.trim();
    let p = std::path::PathBuf::from(target);
    Some(if p.is_absolute() { p } else { repo_path.join(p) })
}

/// Cheap change detector: if none of these moved, the expensive probes
/// (log, reflog, graph, stash, worktree) cannot have new answers. Untracked
/// files do not touch the index, so dirty_count is always probed regardless.
pub async fn work_key(repo_path: &Path) -> Option<WorkKey> {
    let head = run_git(repo_path, &["rev-parse", "HEAD"]).await?;
    if !head.status.success() {
        return None; // unborn HEAD etc: treat as always-changed
    }
    let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();

    let git_dir = resolve_git_dir(repo_path)?;
    // Stash log lives in the COMMON dir for linked worktrees.
    let common_dir = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(rel) => git_dir.join(rel.trim()),
        Err(_) => git_dir.clone(),
    };
    let index_meta = std::fs::metadata(git_dir.join("index")).ok();
    let stash_size = std::fs::metadata(common_dir.join("logs/refs/stash"))
        .map(|m| m.len())
        .unwrap_or(0);

    Some(WorkKey {
        head_sha,
        index_mtime: index_meta.as_ref().and_then(|m| m.modified().ok()),
        index_size: index_meta.map(|m| m.len()).unwrap_or(0),
        stash_size,
    })
}
```

- [ ] **Step 2: Write the failing tests** (in `real_repo_tests`)

```rust
    #[tokio::test]
    async fn work_key_stable_then_changes_on_commit_and_stash() {
        let dir = temp_repo("workkey");

        let k1 = work_key(&dir).await.expect("key");
        let k2 = work_key(&dir).await.expect("key");
        assert_eq!(k1, k2, "key must be stable when nothing changed");

        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        sh(&dir, &["git", "add", "."]);
        sh(&dir, &["git", "commit", "-q", "-m", "second"]);
        let k3 = work_key(&dir).await.expect("key");
        assert_ne!(k1.head_sha, k3.head_sha, "commit must change the key");

        std::fs::write(dir.join("a.txt"), "stash me\n").unwrap();
        sh(&dir, &["git", "stash", "-q"]);
        let k4 = work_key(&dir).await.expect("key");
        assert_ne!(k3.stash_size, k4.stash_size, "stash must change the key");

        std::fs::remove_dir_all(&dir).ok();
    }
```

Run: `cargo test work_key_stable` and confirm it fails (work_key undefined) before Step 1 lands, passes after.

- [ ] **Step 3: Wire into the sweep**

`build_repo_status` sets `work_key: git::work_key(path).await` in its literal. `rebuild_repo_status` becomes two-tier:

```rust
async fn rebuild_repo_status(target: &SweepTarget, author: Option<&str>) -> RepoStatus {
    let path = &target.fs_path;
    let old = &target.old;

    // Cheap tier: always current. Untracked files never touch the WorkKey,
    // so dirty_count must not be skipped.
    let branches = git::branch_statuses(path).await;
    let dirty = git::dirty_count(path).await;
    let remote = git::remote_branches(path).await;
    let branch_names: Vec<String> = branches.iter().map(|b| b.name.clone()).collect();
    let (total_behind, total_ahead) = branches
        .iter()
        .fold((0u32, 0u32), |(b, a), s| (b + s.behind, a + s.ahead));

    let key = git::work_key(path).await;
    let unchanged = key.is_some() && key == old.work_key;

    let (commits, reflog, graph, stash_entries, worktrees) = if unchanged {
        (
            old.commits.clone(),
            old.reflog.clone(),
            old.graph.clone(),
            old.stash_entries.clone(),
            old.worktrees.clone(),
        )
    } else {
        (
            git::commits(path, &format!("-{RECENT_COMMIT_COUNT}"), true, author).await,
            git::reflog(path, REFLOG_COUNT).await,
            git::graph(path, &branch_names, GRAPH_COMMIT_COUNT).await,
            git::stash_list(path).await,
            git::worktree_list(path).await,
        )
    };

    RepoStatus {
        branches,
        dirty,
        behind: total_behind,
        ahead: total_ahead,
        remote_only_branches: git::unfetched_branches(&remote, &branch_names),
        stash_count: stash_entries.len() as u32,
        stash_entries,
        commits,
        reflog,
        graph,
        worktrees,
        work_key: key,
        ..(*old.clone())
    }
}
```

- [ ] **Step 4: Verify**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Manual: `cargo run --release -- --watch --interval 10` on a fleet of ~20+ repos; watch `btop` or Activity Monitor process spawn rate settle after the first sweep (subsequent idle sweeps spawn roughly 4 processes per repo instead of 9+).

- [ ] **Step 5: Stage and present the commit**

```bash
git add src/git.rs src/model.rs src/main.rs
git commit -m "perf: skip expensive probes when a repo's WorkKey is unchanged

HEAD sha + index metadata + stash log size, common-dir aware for linked
worktrees. Idle watch sweeps drop from 9+ to 4 subprocesses per repo."
```

---

### Task 8: Live pending-CI indicator (orange, blinking under watch)

**Files:**
- Modify: `src/ui/badges.rs:87-126` (`badge_spans` signature and CI color), `src/ui/detail.rs` and `src/ui/compact.rs` (call sites of `badge_spans`), `src/ui/detail.rs` CI section color, `README.md` ("What it shows" CI line)

**Interfaces:**
- Produces: `pub fn badge_spans(repo: &RepoStatus, dim_pending: bool) -> (Vec<Span<'static>>, usize)`. Callers pass `app.watch_mode && !app.blink_on` (both views hold `&App`).
- Consumes: `app.blink_on` from Task 3, live CI data from Task 6's GitHub re-poll.

- [ ] **Step 1: Change the badge color logic**

In `badge_spans`, replace the CI color match (ui/badges.rs:98-103):

```rust
        // Pending is orange; under watch it blinks (dim phase drops to
        // SURFACE, same glyph, so column widths never jitter). Passed and
        // Failed stay steady.
        let ci_color = match repo.ci_status {
            CiStatus::Passed => colors::GREEN,
            CiStatus::Failed => colors::RED,
            CiStatus::Pending => {
                if dim_pending {
                    colors::SURFACE
                } else {
                    colors::PEACH
                }
            }
            CiStatus::Unknown => colors::OVERLAY,
        };
```

Update every `badge_spans(repo)` call site in ui/detail.rs and ui/compact.rs to `badge_spans(repo, app.watch_mode && !app.blink_on)`. In the detail pane's CI section, change the Pending line color from YELLOW to PEACH (steady there; the blink lives in the badge only).

- [ ] **Step 2: Verify**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Manual: push a commit to a repo with a GitHub Action, run `cargo run --release -- --watch --interval 15`, and confirm: the CI badge shows orange and pulses at ~1Hz while the action runs (watch on), goes steady green or red when it finishes (via the Task 6 re-poll), and does not blink when watch is off.

- [ ] **Step 3: Update README**

"What it shows" CI bullet becomes: `CI status (green check, red cross, blinking orange circle while running) from GitHub's statusCheckRollup, kept live in watch mode`. Move the now-implemented Roadmap bullets (background refresh, fetch in the refresh cycle, tick-based redraw, GitHub re-polling) out of Roadmap; leave the notify/ETag bullets, annotated as the next step.

- [ ] **Step 4: Stage and present the commit**

```bash
git add src/ui/badges.rs src/ui/detail.rs src/ui/compact.rs README.md
git commit -m "feat: pending CI badge is orange and blinks while watch mode is live"
```

---

## Self-Review (performed while writing)

- Spec coverage: instant open (Task 4), never-blocking queries with indication (Tasks 3, 4, 6 spinner and swept-ago), watch actually updates everything (Task 6 full rebuild fixes the 5-field write-back; fetch in sweep fixes frozen behind; GitHub re-poll fixes frozen PR/CI), performance architecture (Tasks 5, 7), blinking orange live CI (Task 8), git hardening and repoz-parity fixes (Tasks 1, 2). The old silent `if let Some(last)` watch-death trap is closed by `map_or(true, ...)` in Task 6 Step 3.
- Placeholder scan: no TBDs; the one repetitive section (Task 1 Step 3) lists exact argument arrays per function rather than "similar to above".
- Signature consistency: `App::new` 5-param form (Task 2) is what Tasks 3-4 use; `badge_spans(repo, dim_pending)` matches Task 8's callers; `AppEvent` variants defined in Task 3 are exactly the ones emitted in Tasks 4-6; `run_git` return contract (Option<Output>) is respected by Tasks 2 and 7; `rebuild_repo_status` is introduced in Task 6 and refined in Task 7 with the same signature.
