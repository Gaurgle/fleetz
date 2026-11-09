use crate::app::App;
use crate::cli::Args;
use crate::engine::{carries, names_remote};
use crate::event::AppEvent;
use crate::load::{assess_size_holds, spawn_memory_rebuild};
use crate::model::RepoStatus;
use crate::sweep::{order_rows_by_activity, start_single_sweep};
use crate::{app, cache, ci, config, digest, git, github, model, notes, ui};
use std::path::Path;

/// Same idea for the GitHub poll. The HTTP client has no timeout (`gh` only
/// supplies the token), so a black-holed request would otherwise hold
/// `github_poll_inflight` for the rest of the session and no poll could start.
/// Past this the poll is given up on and recorded as a timed-out failure,
/// which the header shows (`expire_stuck_github_poll`).
pub(crate) const GITHUB_POLL_STUCK_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

/// Same idea for a CI poll (`App::expire_stuck_ci_polls`). Longer than the
/// fleet's, since one poll is up to three calls in a row (list, jobs, log),
/// each bounded at 30 s by the shared client, after a token lookup bounded at
/// 10 s: past this one is wedged, not slow.
pub(crate) const CI_POLL_STUCK_AFTER: std::time::Duration = std::time::Duration::from_secs(120);

/// Floor on the GitHub poll interval, whatever the config says, and the
/// interval while a repo is `Pending`. One poll at `repo_limit` 50 costs 3
/// points, so one instance at the floor spends 720 of the 5000-point hour:
/// comfortable for one instance, not for several on one account, which reach
/// the limit at seven (`docs/api-budget-design.md` section 3). The floor
/// stops a typo; the budget pacing (`app::ApiBudget::pace`) is what slows
/// polling when the account runs low.
const GITHUB_POLL_MIN: u64 = 15;

/// Whether a sweep result means the repo moved in a way CI might care about,
/// and the next GitHub poll should not wait out its interval.
///
/// Two signals, because neither alone is enough:
///
/// - **HEAD moved.** Catches the commit. Note a `git push` does *not* move
///   HEAD, so this misses a push of commits made before fleetz was watching.
/// - **Unpushed count fell to zero.** Catches that push. But it only fires if
///   an earlier sweep actually observed the "committed, not yet pushed" state,
///   which it will not when you commit and push inside one sweep interval --
///   the common case, and the reason the first version of this missed.
///
/// Together they cover commit-then-push at any speed. HEAD is compared only
/// when both sweeps produced a key, so an unknown fingerprint reads as "no
/// information" rather than as a change, which would poll on every sweep.
pub(crate) fn repo_moved(before: &RepoStatus, after: &RepoStatus) -> bool {
    let head_moved = match (&before.work_key, &after.work_key) {
        (Some(a), Some(b)) => a.head_sha != b.head_sha,
        _ => false,
    };
    let pushed = before.ahead > 0 && after.ahead == 0;
    head_moved || pushed
}

/// Land a GitHub poll's result (`AppEvent::Github`).
///
/// A failure changes no row and leaves the data time alone: what is in memory
/// is at least as new as the cache file, so the fallback the fetch attaches is
/// not applied here (contract rule 4). Only launch and `R` use it.
pub(crate) fn apply_github_poll(
    app: &mut App,
    attempt: u64,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    author: &Option<String>,
) {
    apply_github_poll_at(app, attempt, result, tx, author, chrono::Local::now());
}

/// `apply_github_poll` with the arrival time passed in, so a test can plant
/// the clock the budget reading and the not-before are anchored to.
pub(crate) fn apply_github_poll_at(
    app: &mut App,
    attempt: u64,
    result: Result<Box<github::GithubFetch>, github::FetchError>,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    author: &Option<String>,
    now: chrono::DateTime<chrono::Local>,
) {
    // Whether or not the answer is applied, its headers are true about the
    // account at the moment the server wrote them (design 4.1).
    let reading = match &result {
        Ok(fetch) => fetch.meta.budget.clone(),
        Err(e) => e.meta.budget.clone(),
    };
    // The in-flight lock belongs to the newest attempt. A late answer from
    // one given up on must not release it, or a second request stacks; the
    // newest one's answer always does, even when it is discarded below.
    let is_latest = app.github_freshness.is_latest(attempt);
    if is_latest {
        app.github_poll_inflight = false;
        app.github_poll_started = None;
    }
    // An attempt given up on can still answer, and an `R` can land while a
    // poll is out. Once a newer answer has landed, applying this one would
    // move the data backwards (rule 10).
    if !app.github_freshness.accept_answer(attempt) {
        // Its reading may still lower `remaining` on the held token, never
        // switch it; its outcome changes no count and no not-before.
        app.observe_budget(reading.as_ref(), false, now);
        if is_latest {
            // A refresh waiting on this request is over all the same.
            app.settle_refresh_poll(std::iter::empty());
        }
        return;
    }
    app.observe_budget(reading.as_ref(), true, now);
    // Read before the outcome below replaces it: a recovery from `Offline`
    // refetches the rows the outage failed (fetch contract rule 6).
    let previous_outcome = app.github_freshness.outcome.clone();
    match result {
        Ok(fetch) => {
            app.github_freshness.succeeded(now);
            app.github_backoff.succeeded();
            let github::GithubFetch {
                repos: dataset,
                calendar,
                has_pr_counts: _,
                // The poll always bypasses the cache, so this is
                // network data by construction.
                origin: _,
                meta,
            } = *fetch;
            // Before the meta is replaced: the rows were built on the account
            // the meta held until now.
            if account_rebuild_needed(app.github_meta.account.as_deref(), meta.account.as_deref()) {
                app.account_rebuild_due = true;
            }
            // The dataset is replaced below, so its coverage is too: the next
            // window apply draws its note from what this poll covered.
            app.github_meta = meta;
            // The calendar rides the same payload as the repos, so
            // a poll that updated CI also refreshed the year at no
            // extra cost. A payload predating the field leaves the
            // last good calendar in place rather than blanking it.
            // A payload predating the field leaves the last good
            // calendar in place rather than blanking it.
            if calendar.is_some() {
                app.github_calendar = calendar;
            }
            // Overwrite only the GitHub-owned fields: the local
            // git state is the sweep's business, not this poll's.
            // Only on the row the load made the carrier
            // (`carries`), so a same-named row of another owner, or
            // a local-only checkout, never takes these.
            // A carrier row the poll did not return keeps its values and is
            // marked as from an earlier refresh; one it returned loses the
            // mark (design 4.7).
            for repo in &mut app.repos {
                let entry = dataset.iter().find(|g| carries(repo, g));
                if let Some(gh) = entry {
                    repo.apply_github(gh);
                }
                if let Some(key) = app::carrier_key(repo) {
                    if entry.is_some() {
                        app.not_in_last_poll.remove(&key);
                    } else {
                        app.not_in_last_poll.insert(key);
                    }
                }
            }
            // One API call just told us which remotes moved, so
            // fetch only those instead of waiting for the next
            // blanket sweep to fetch all of them.
            let mut moved = remotes_that_moved(&app.dataset, &dataset, &app.repos);
            // The outage's failures ride the same batch, each row once.
            for idx in
                recovery_refetch(&previous_outcome, &app.github_freshness.outcome, &app.repos)
            {
                if !moved.contains(&idx) {
                    moved.push(idx);
                }
            }
            // The new sizes against each row's baseline, before choosing.
            assess_size_holds(&mut app.repos, &dataset, app.fetch_max_bytes);
            // A row held after a timeout waits out its hold (FZ-34), and one
            // the fetch cap holds waits for `f` (FZ-35b).
            moved.retain(|&idx| {
                let repo = &app.repos[idx];
                repo.fetch.size_hold().is_none()
                    && repo
                        .path
                        .as_deref()
                        .is_none_or(|path| !app.fetch_held(path, now))
            });
            for &idx in &moved {
                start_single_sweep(app, tx, idx, true, true, author.clone());
            }
            // A manual `r` keeps spinning until these land.
            let fetched_paths: Vec<String> = moved
                .iter()
                .filter_map(|&idx| app.repos[idx].path.clone())
                .collect();
            if is_latest {
                app.settle_refresh_poll(fetched_paths);
            }
            app.dataset = dataset;
            // After the indices above are spent: this moves rows.
            order_rows_by_activity(app);
        }
        Err(e) => {
            app.set_status(
                format!("GitHub refresh failed: {}", e.failure.message),
                true,
            );
            app.github_attempt_failed(&e.failure, now);
            app.github_freshness.failed(e.failure, now);
        }
    }
    // A failed poll fetched nothing, so a refresh waiting on it
    // is over. No-op when the Ok arm already settled it.
    if is_latest {
        app.settle_refresh_poll(std::iter::empty());
    }
    app.needs_redraw = true;
}

/// The rows fetched once because the GitHub poll recovered (fetch contract
/// rule 6): when `previous` failed as `Offline` and `current` succeeded,
/// every row with a path whose last fetch failed as `Unreachable` or
/// `TimedOut`. Any other pair selects nothing, so the refetch never repeats
/// on its own, and a row marked for any other reason is left alone.
fn recovery_refetch(
    previous: &app::Outcome,
    current: &app::Outcome,
    repos: &[RepoStatus],
) -> Vec<usize> {
    let recovered = matches!(
        previous,
        app::Outcome::Failed(f) if f.kind == github::FailureKind::Offline
    ) && *current == app::Outcome::Success;
    if !recovered {
        return Vec::new();
    }
    repos
        .iter()
        .enumerate()
        .filter(|(_, r)| r.path.is_some())
        .filter(|(_, r)| {
            r.fetch.failure().is_some_and(|f| {
                matches!(
                    f.kind,
                    git::FetchFailureKind::Unreachable | git::FetchFailureKind::TimedOut
                )
            })
        })
        .map(|(i, _)| i)
        .collect()
}

/// Whether an applied successful poll on account `poll` must rebuild rows
/// built on account `rows` (design 4.7, O5). Only a poll whose account is
/// known decides: it rebuilds when the rows' account differs or is unknown
/// (a launch from the legacy file or under the signed-out rule). A poll with
/// no account never does. A token rotation on one account changes the key
/// and costs one rebuild from that account's own data, which is accepted.
fn account_rebuild_needed(rows: Option<&str>, poll: Option<&str>) -> bool {
    poll.is_some() && rows != poll
}

/// Rebuild the rows from the dataset in memory after an account change,
/// once nothing else is loading: through the rebuild from memory, with no
/// request, no `git fetch`, no `window.toml` write and no "Window:" status.
/// While a load runs the flag waits; a load that fetched clears it.
pub(crate) fn start_account_rebuild(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
) {
    if !app.account_rebuild_due || app.loading {
        return;
    }
    app.account_rebuild_due = false;
    app.loading = true;
    spawn_memory_rebuild(tx, app, args, config, repo_dir, false);
}

/// Give up on a background poll that has not answered within `limit`.
///
/// Recorded as a timed-out failure, which also advances the retry clock: the
/// in-flight flag used to be dropped with nothing else changed, so the next
/// tick started a second request while the first was still open (decision
/// D4). The request itself is not cancelled; that is FZ-11.
pub(crate) fn expire_stuck_github_poll(
    app: &mut App,
    limit: std::time::Duration,
    now: chrono::DateTime<chrono::Local>,
) {
    if let Some(started) = app.github_poll_started
        && started.elapsed() > limit
    {
        app.github_poll_inflight = false;
        app.github_poll_started = None;
        app.refresh_awaits_poll = false;
        app.github_freshness.gave_up(limit, now);
        // A give-up is a `TimedOut` failure and climbs the ladder like one
        // (transition 5).
        if let app::Outcome::Failed(failure) = app.github_freshness.outcome.clone() {
            app.github_attempt_failed(&failure, now);
        }
        app.needs_redraw = true;
    }
}

/// Whether the tick should start a background GitHub poll now.
///
/// With watch on, once per `github_poll_secs` after the last attempt,
/// whatever it ended in, or sooner for a detected push while polls succeed.
/// With watch off, only the one attempt a cache-served launch is owed
/// (decision D2): the data on screen came off the disk and nothing has
/// confirmed it yet.
pub(crate) fn github_poll_due(
    app: &App,
    config: &config::Config,
    now: chrono::DateTime<chrono::Local>,
) -> bool {
    if app.dirty_mode || app.loading || app.github_poll_inflight {
        return false;
    }
    let freshness = &app.github_freshness;
    if !app.watch_mode {
        return freshness.awaits_first_attempt();
    }
    // Beside the interval, not inside it: a rate limit's reset, a
    // `retry-after`, a backoff step or a pause at the reserve.
    let held = github_not_before(app, config).is_some_and(|t| now < t);
    // A detected push jumps the queue: it is the one moment CI is about to
    // change with nothing `Pending` yet to notice. Not while polls fail, where
    // the next attempt waits out at least a full interval (rule 6), nor while
    // the budget slows polling; the next due poll serves it then.
    let slowed = app.budget_slowed(
        configured_poll_secs(config),
        github::poll_cost(config.repo_limit),
    );
    let push = app.push_detected && !freshness.is_failing() && !slowed;
    !held && (push || freshness.retry_due(now, github_poll_secs(app, config)))
}

/// The configured poll interval, floored.
pub(crate) fn configured_poll_secs(config: &config::Config) -> i64 {
    config.github_poll_interval.max(GITHUB_POLL_MIN) as i64
}

/// The earliest the next automatic attempt may go besides the interval: the
/// later of the retry ladder's not-before and a pause at the reserve.
fn github_not_before(
    app: &App,
    config: &config::Config,
) -> Option<chrono::DateTime<chrono::Local>> {
    let ladder = app.github_backoff.not_before(configured_poll_secs(config));
    let pause = match app.github_pace(github::poll_cost(config.repo_limit)) {
        app::Pace::Paused { until } => Some(until),
        _ => None,
    };
    ladder.max(pause)
}

/// `App::github_limit_until` under `config`.
pub(crate) fn github_limit_until(
    app: &App,
    config: &config::Config,
    now: chrono::DateTime<chrono::Local>,
) -> Option<chrono::DateTime<chrono::Local>> {
    app.github_limit_until(
        configured_poll_secs(config),
        github::poll_cost(config.repo_limit),
        now,
    )
}

/// The status line for a refresh that sent nothing because of a known limit.
/// Provisional words; FZ-10c settles them.
pub(crate) fn limited_status(until: chrono::DateTime<chrono::Local>) -> String {
    format!("GitHub rate limited, resumes at {}", until.format("%H:%M"))
}

/// Indices into `repos` whose remote moved between two GitHub polls.
///
/// This inverts the cost of noticing someone else's push. Discovering it by
/// fetching means a network round trip per remote, every sweep, almost always
/// to learn that nothing changed. One GraphQL call returns `pushed_at` for
/// every repo at once, so the poll can say *which* remotes moved and only
/// those need fetching.
///
/// Note this is the opposite conclusion to per-repo CI queries, for a
/// consistent reason: GraphQL batches cheaply, so ask about everything at
/// once; git fetch is priced per remote, so only fetch where you know there
/// is news.
///
/// A repo missing from the previous dataset yields nothing: on the first poll
/// of a session there is no baseline, and treating "unknown" as "moved" would
/// fetch the entire fleet at once. Repos with no local clone are skipped too,
/// since there is nothing to fetch.
///
/// Only the carrier of a moved remote is fetched (`carries`), the row the poll
/// writes. An extra checkout of the same remote is not; `F` still reaches it.
///
/// The previous entry is the side `names_remote` matches from, so an owned
/// one is only ever the same owner's push, and one without an owner (a
/// launch from a payload without `login`) still matches by name. That keeps
/// the legacy baseline: unmatched, a push since it would read as unknown and
/// never be fetched, since the next poll compares against owned data.
fn remotes_that_moved(
    previous: &[github::GithubRepo],
    current: &[github::GithubRepo],
    repos: &[RepoStatus],
) -> Vec<usize> {
    current
        .iter()
        .filter(|now| {
            previous
                .iter()
                .find(|was| names_remote(was.owner.as_deref(), &was.name, now))
                .is_some_and(|was| now.pushed_at > was.pushed_at)
        })
        .filter_map(|moved| {
            repos
                .iter()
                .position(|r| r.path.is_some() && carries(r, moved))
        })
        // Each row once, in the order its first moved entry came: two entries
        // can resolve to one row (an owner-less legacy row carries every
        // same-named entry), and a second index is a second single sweep.
        .fold(Vec::new(), |mut rows, idx| {
            if !rows.contains(&idx) {
                rows.push(idx);
            }
            rows
        })
}

/// Seconds to wait before the next GitHub poll.
///
/// Drops to the floor while any repo is `Pending`, which is exactly when a
/// human is watching the screen for a run to finish, and relaxes to the
/// configured interval when nothing is in flight. Costs nothing at idle.
///
/// Not while polls fail (decision D3): a `Pending` frozen by a failing poll
/// says nothing about what CI is doing now, and the floor would only retry a
/// dead network four times a minute. Nor while the budget slows polling
/// (A2): then the pace, when longer than the configured interval, is the
/// wait. A backoff step, a limit and a pause are not here but in
/// `github_not_before`, which `github_poll_due` checks beside this.
fn github_poll_secs(app: &App, config: &config::Config) -> i64 {
    let configured = configured_poll_secs(config);
    let cost = github::poll_cost(config.repo_limit);
    let pace = match app.github_pace(cost) {
        app::Pace::Every(secs) => secs,
        app::Pace::Unknown | app::Pace::Paused { .. } => 0,
    };
    // A row the last poll did not return holds a `Pending` frozen at an
    // earlier refresh, which says nothing about CI now (design 4.7).
    let any_pending = app
        .repos
        .iter()
        .any(|r| r.ci_status == model::CiStatus::Pending && !app.is_not_in_last_poll(r));
    let floored =
        any_pending && !app.github_freshness.is_failing() && !app.budget_slowed(configured, cost);
    let interval = if floored {
        GITHUB_POLL_MIN as i64
    } else {
        configured
    };
    interval.max(pace)
}

/// Kick off a GitHub re-poll, taking the in-flight lock. No-op if one is
/// already running, so mashing `r` cannot stack polls. `R` does not come
/// through here: it is a full load, and `reload_action` ignores it while a
/// load runs (A7), which is what keeps it from stacking requests.
pub(crate) fn start_github_poll(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    config: &config::Config,
) {
    if let Some(attempt) = claim_github_poll(app) {
        spawn_github_poll(tx, config, attempt);
    }
}

/// The tick's CI work: give up on a stuck poll, then start the one that is
/// due. Both only while pane 2 shows `ci` (`App::ci_wanted`); elsewhere,
/// `info` included, the CI data layer is left alone, and what it holds
/// stays for the next visit.
pub(crate) fn ci_tick(
    app: &mut App,
    now: chrono::DateTime<chrono::Local>,
    instant: std::time::Instant,
) -> Option<ci::CiRequest> {
    if !app.ci_wanted() {
        return None;
    }
    app.expire_stuck_ci_polls(CI_POLL_STUCK_AFTER, instant, now);
    ci_poll_request(app, now, instant)
}

/// The CI poll the tick should start for the selected repo, with its lock
/// taken, or `None`. Nothing unless pane 2 shows `ci`, nothing under `-d`
/// or on a row without a GitHub identity; otherwise `ci::poll_due` decides,
/// from the cursor's dwell and the repo's cadence.
pub(crate) fn ci_poll_request(
    app: &mut App,
    now: chrono::DateTime<chrono::Local>,
    instant: std::time::Instant,
) -> Option<ci::CiRequest> {
    if !app.ci_wanted() || app.dirty_mode {
        return None;
    }
    let target = app.selected_repo().and_then(ci::target);
    let settled_for = app.ci_settled_for(target.as_ref().map(|(key, ..)| key), instant);
    let (key, owner, name) = target?;
    // The poll reads the highlighted run's jobs (`CiState::expanded`).
    app.sync_ci_inspector(instant);
    ci::poll_due(&app.ci_cadence(&key, settled_for, now))
        .then(|| app.claim_ci_poll(key, owner, name))
}

/// Run a claimed CI poll in the background and send its answer back.
pub(crate) fn spawn_ci_poll(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    request: ci::CiRequest,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let answer = ci::poll(request).await;
        let _ = tx.send(AppEvent::Ci(Box::new(answer)));
    });
}

/// Run a claimed inspector fetch in the background and send its answer back.
pub(crate) fn spawn_ci_detail(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    request: ci::DetailRequest,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let answer = ci::fetch_detail(request).await;
        let _ = tx.send(AppEvent::CiDetail(Box::new(answer)));
    });
}

/// Read a worktree's newest commits for the `worktrees` inspector off the
/// UI thread (FZ-77): one bounded `git log` on `HEAD` in that directory.
pub(crate) fn spawn_worktree_log(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    read: app::WorktreeLogRead,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let commits = git::branch_commits(&read.dir, "HEAD", app::WORKTREE_LOG_COUNT).await;
        let _ = tx.send(AppEvent::WorktreeLogRead { read, commits });
    });
}

/// Walk a repo for the `notes` view off the UI thread and send the tree back.
pub(crate) fn spawn_notes_walk(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    walk: app::NotesWalk,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let root = walk.root.clone();
        let home = std::path::PathBuf::from(shellexpand::tilde("~").as_ref());
        let result = tokio::task::spawn_blocking(move || {
            notes::walk(&root).map(|n| n.with_personal(&root, notes::personal_board(&home, &root)))
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
        let _ = tx.send(AppEvent::NotesWalked { walk, result });
    });
}

/// Read a repo's `changes` off the UI thread and send them back (FZ-60).
pub(crate) fn spawn_changes_read(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    read: app::ChangesRead,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = git::changes(&read.root, read.base).await;
        let _ = tx.send(AppEvent::ChangesRead { read, result });
    });
}

/// Read the highlighted changed file's diff off the UI thread and send it
/// back (FZ-60).
pub(crate) fn spawn_change_diff(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    read: app::DiffRead,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let text = git::changed_file_diff(&read.root, read.base.as_deref(), &read.file).await;
        let _ = tx.send(AppEvent::ChangeDiff { read, text });
    });
}

/// Read the highlighted note off the UI thread and send its body back with
/// its metadata for the facts (FZ-62), then its last commit from a bounded
/// `git log`, "untracked" when git prints nothing. Two answers, body first:
/// for a file git never saw it walks the whole history, and the reader
/// shows the body without waiting for that. A markdown body is rendered in
/// the same blocking task at the read's `render_width` (FZ-59): a capped
/// file costs more than a keypress frame.
pub(crate) fn spawn_note_read(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    read: app::NoteRead,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let path = read.path.clone();
        let render_width = read.render_width;
        let read_both = tokio::task::spawn_blocking(move || {
            let text = notes::read_body(&path);
            let rendered = match (&text, render_width) {
                (Ok(body), Some(width)) => Some(render_note(body, width)),
                _ => None,
            };
            (text, notes::file_facts(&path), rendered)
        })
        .await;
        let (text, facts, rendered) = match read_both {
            Ok(answer) => answer,
            Err(e) => (Err(e.to_string()), notes::NoteFacts::default(), None),
        };
        let _ = tx.send(AppEvent::NoteRead {
            read: read.clone(),
            text,
            facts,
            rendered,
        });
        let rel = read.path.strip_prefix(&read.root).unwrap_or(&read.path);
        let rel = rel.to_string_lossy();
        // Literal, so a name with `:` or glob characters is a path, not
        // pathspec magic.
        let args = [
            "--literal-pathspecs",
            "log",
            "-1",
            "--format=%cI",
            "--",
            &rel,
        ];
        let last = git::run_git(&read.root, &args)
            .await
            .filter(|out| out.status.success())
            .map_or(notes::LastCommit::Untracked, |out| {
                notes::last_commit_from(&String::from_utf8_lossy(&out.stdout))
            });
        let _ = tx.send(AppEvent::NoteCommit { read, last });
    });
}

/// `body` as the reader draws it rendered at `width` (FZ-59).
/// A body `notes::read_body` cut at the cap ends in its marker line, which
/// is not the note's markdown: it is rendered without it and drawn after
/// it, plain, as the raw reader draws it, so an open fence or table cannot
/// swallow it.
fn render_note(body: &str, width: u16) -> app::RenderedNote {
    let marker = format!("… cut at {} KiB", notes::BODY_CAP / 1024);
    let suffix = format!("\n{marker}");
    let (markdown, cut) = match body.strip_suffix(&suffix) {
        Some(markdown) => (markdown, true),
        None => (body, false),
    };
    let mut lines = ui::markdown::render_markdown(markdown, width);
    if cut {
        let plain = ratatui::style::Style::default().fg(ui::colors::TEXT);
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            marker, plain,
        )));
    }
    app::RenderedNote { width, lines }
}

/// Render the held markdown body again at a new reader width, off the UI
/// thread, and send it back (`App::claim_note_render`).
pub(crate) fn spawn_note_render(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    render: app::NoteRender,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let rendered = tokio::task::spawn_blocking(move || {
            let rendered = render_note(&render.text, render.width);
            (render, rendered)
        })
        .await;
        if let Ok((render, rendered)) = rendered {
            let _ = tx.send(AppEvent::NoteRendered { render, rendered });
        }
    });
}

/// Run a slot digest's `git log --since` in the background (FZ-45) and send
/// the commits back: never on the UI thread.
pub(crate) fn spawn_digest_log(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    request: digest::LogRequest,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let commits = git::commits_since(&request.dir, request.key.cutoff).await;
        let _ = tx.send(AppEvent::DigestLog {
            path: request.path,
            key: request.key,
            commits,
        });
    });
}

/// Take the in-flight lock and an attempt id for a new poll, or `None` when
/// one is already running or `-d` has no GitHub to poll.
pub(crate) fn claim_github_poll(app: &mut App) -> Option<u64> {
    if app.github_poll_inflight || app.dirty_mode {
        return None;
    }
    app.github_poll_inflight = true;
    app.github_poll_started = Some(std::time::Instant::now());
    Some(app.github_freshness.begin_attempt())
}

/// Re-poll GitHub for PR and CI data in the background, so badges stop being
/// frozen at whatever they were when fleetz launched.
fn spawn_github_poll(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    config: &config::Config,
    attempt: u64,
) {
    let tx = tx.clone();
    let repo_limit = config.repo_limit;
    // PR data is always fetched now, so the cache key no longer varies by it.
    let cache = cache::KeyedCache::new(repo_limit, github::HOST);
    tokio::spawn(async move {
        let result = github::fetch_github(repo_limit, &cache, github::CacheMode::Bypass).await;
        let _ = tx.send(AppEvent::Github {
            attempt,
            result: result.map(Box::new),
        });
    });
}

#[cfg(test)]
mod tests;
