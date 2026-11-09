use crate::app::App;
use crate::cli::Args;
use crate::engine::{
    LoadError, ViewNotes, assemble_view, disk_usage_of, load_repos, resolve_cutoff, size_held_rows,
};
use crate::event::{AppEvent, LoadKind, LoadResult};
use crate::model::RepoStatus;
use crate::poll::{github_limit_until, limited_status};
use crate::{app, config, github, model, time, ui, window_state};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Assign each repo to the config-defined groups whose member list names it.
/// Clears first so it's safe to re-run after a reload replaces the list.
fn apply_groups(repos: &mut [RepoStatus], config: &config::Config) {
    for repo in repos.iter_mut() {
        repo.groups.clear();
        for (group_name, members) in &config.groups {
            if members.iter().any(|m| m.eq_ignore_ascii_case(&repo.name)) {
                repo.groups.push(group_name.clone());
            }
        }
    }
}

/// Run the full `load_repos` (GitHub fetch + discovery + git status) on a
/// background task and deliver the result as `AppEvent::Loaded`, so the UI
/// keeps painting and accepting keys throughout. `bypass_cache` is what makes
/// `R` see a just-pushed repo. Not for window applies: those are a pure
/// re-filter and go through `spawn_window_apply`.
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
pub(crate) fn spawn_load(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    window: Option<time::TimeWindow>,
    kind: LoadKind,
    bypass_cache: bool,
    fetch_claims: &app::FetchClaims,
    size_held: HashSet<String>,
) {
    debug_assert!(
        !matches!(kind, LoadKind::WindowApply(_) | LoadKind::FromMemory),
        "window applies and rebuilds from memory fetch nothing"
    );
    let tx = tx.clone();
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    let fetch_claims = fetch_claims.clone();
    tokio::spawn(async move {
        let mode = args.mode();
        let result = load_repos(
            &args,
            &config,
            &repo_dir,
            &mode,
            window,
            bypass_cache,
            &fetch_claims,
            &size_held,
        )
        .await
        .map(|(repos, notes, dataset, calendar, source, meta)| {
            Box::new(LoadResult {
                kind,
                repos,
                header: notes.header,
                dataset: Some(dataset),
                github_calendar: calendar,
                source,
                ambiguity: notes.ambiguity,
                meta,
            })
        });
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

/// Re-filter the already-fetched dataset against a newly chosen time window,
/// on a background task. No git fetch and no GitHub call, so this is cheap;
/// it still reports statting progress so the footer gauge animates.
pub(crate) fn spawn_window_apply(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    app: &App,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    w: time::TimeWindow,
) {
    let tx = tx.clone();
    let dataset = app.dataset.clone();
    // The note is drawn from the coverage of the dataset being re-filtered.
    let coverage = app.github_meta.coverage.clone();
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    let fetch_claims = app.fetches_in_flight.clone();
    tokio::spawn(async move {
        let cutoff = w.cutoff(config.work_start, config.work_end);
        let header = w.label(config.work_start, config.work_end);
        let result = assemble_view(
            &dataset,
            coverage.as_ref(),
            Some(cutoff),
            header,
            &args,
            &config,
            &repo_dir,
            false,
            Some(&tx),
            &fetch_claims,
            &HashSet::new(),
        )
        .await
        .map(|(repos, notes)| Box::new(window_apply_result(w, repos, notes)))
        .map_err(LoadError::from);
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

/// Rediscover and re-sweep over the dataset already in memory instead of
/// asking GitHub. On a background task like any load. `R` under a known rate
/// limit (A6) passes `git_fetch` true, for the git fetches `R` has; the
/// account rebuild (design 4.7) passes false.
pub(crate) fn spawn_memory_rebuild(
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    app: &App,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    git_fetch: bool,
) {
    let tx = tx.clone();
    let dataset = app.dataset.clone();
    let coverage = app.github_meta.coverage.clone();
    let window = app.window;
    let args = args.clone();
    let config = config.clone();
    let repo_dir = repo_dir.to_path_buf();
    let fetch_claims = app.fetches_in_flight.clone();
    let size_held = size_held_rows(&app.repos);
    tokio::spawn(async move {
        let mode = args.mode();
        let result = match resolve_cutoff(window, &mode, &config) {
            Ok((cutoff, header)) => assemble_view(
                &dataset,
                coverage.as_ref(),
                cutoff,
                header,
                &args,
                &config,
                &repo_dir,
                git_fetch,
                None,
                &fetch_claims,
                &size_held,
            )
            .await
            .map(|(repos, notes)| Box::new(memory_rebuild_result(repos, notes)))
            .map_err(LoadError::from),
            Err(e) => Err(LoadError::from(e)),
        };
        let _ = tx.send(AppEvent::Loaded(result));
    });
}

/// What a rebuild from memory delivers: rows and notes, nothing fetched from
/// GitHub, so the dataset, calendar and coverage in the app stay as they are.
pub(crate) fn memory_rebuild_result(repos: Vec<RepoStatus>, notes: ViewNotes) -> LoadResult {
    LoadResult {
        kind: LoadKind::FromMemory,
        repos,
        header: notes.header,
        dataset: None,
        github_calendar: None,
        // No attempt: data time, last attempt and outcome stay what the last
        // real attempt left (amendment 5).
        source: app::DatasetSource::Local,
        ambiguity: notes.ambiguity,
        meta: None,
    }
}

/// What a window apply delivers: rows and notes, and nothing fetched.
fn window_apply_result(
    w: time::TimeWindow,
    repos: Vec<RepoStatus>,
    notes: ViewNotes,
) -> LoadResult {
    LoadResult {
        kind: LoadKind::WindowApply(w),
        repos,
        header: notes.header,
        dataset: None,
        github_calendar: None,
        // A window apply re-filters what is already in memory and
        // fetches nothing, so it must not touch the poll clock either
        // way. `dataset: None` already guards that.
        source: app::DatasetSource::Local,
        // Discovery runs again, so the ambiguity rides along like any
        // load's.
        ambiguity: notes.ambiguity,
        // Nothing was fetched: the app keeps the dataset's coverage, which
        // the header note above was drawn from.
        meta: None,
    }
}

/// Swap a finished load into the app, preserving UI state (selection is
/// clamped, groups re-applied), and say what happened.
pub(crate) fn apply_load_result(
    app: &mut App,
    config: &config::Config,
    result: Result<Box<LoadResult>, LoadError>,
) {
    apply_load_result_saving_to(app, config, result, &window_state::WindowState::new());
}

/// `apply_load_result` with the store a window apply persists its window to
/// passed in, so a test can land a window apply without writing the user's
/// saved window.
pub(crate) fn apply_load_result_saving_to(
    app: &mut App,
    config: &config::Config,
    result: Result<Box<LoadResult>, LoadError>,
    window_store: &window_state::WindowState,
) {
    let now = chrono::Local::now();
    app.apply_progress = None;
    app.loading = false;
    match result {
        Ok(lr) => {
            app.load_error = None;
            // A fetch the load ran (launch, `R`) counts toward the timeout
            // hold like a sweep's: only such a row has an attempt.
            for repo in &lr.repos {
                if let (Some(path), Some(at)) = (repo.path.as_deref(), repo.fetch.last_attempt) {
                    app.fetch_backoff.record(path, &repo.fetch.outcome, at);
                }
            }
            app.repos = carry_fetch_facts(&app.repos, lr.repos);
            // Every row now comes from one dataset, so none is "not in" it
            // (design 4.7): a launch, `R`, a window apply, a rebuild from
            // memory and an account rebuild alike.
            app.not_in_last_poll.clear();
            // A load that fetched rebuilt the rows from that fetch's account,
            // which a pending account rebuild would only repeat; a later poll
            // on another account asks again.
            if lr.meta.is_some() {
                app.account_rebuild_due = false;
            }
            apply_groups(&mut app.repos, config);
            app.header = lr.header;
            if let Some(ds) = lr.dataset {
                let reading = lr.meta.as_ref().and_then(|m| m.budget.as_ref());
                record_load_attempt(app, &lr.source, reading, now);
                app.dataset = ds;
                // Only an attempt may start the re-poll clock. Startup serves
                // any cached payload however old (`CacheMode::AnyCached`), and
                // starting the clock from that left the first real poll a full
                // `github_poll_interval` away: every launch opened on whatever
                // the last session had left on disk and sat there for thirty
                // seconds, with nothing on screen to say the numbers were not
                // current. A cache read leaves it unset, so the poll is due on
                // the first tick instead.
                app.github_freshness.loaded(&lr.source, now);
            }
            // The rows are new and so may be the dataset: the pre-check
            // reads both (FZ-35b).
            assess_size_holds(&mut app.repos, &app.dataset, app.fetch_max_bytes);
            // Only a load that fetched carries one; a window apply keeps the
            // coverage its dataset came with.
            if let Some(meta) = lr.meta {
                app.github_meta = meta;
            }
            // The calendar rides the same fetch as the dataset. Dropping it
            // here left the strip blank until the first background poll
            // landed, minutes later, which read as the feature being broken.
            // A WindowApply carries None and must not blank a good calendar.
            if lr.github_calendar.is_some() {
                app.github_calendar = lr.github_calendar;
            }
            app.clamp_after_reload();
            app.last_refreshed = Some(chrono::Local::now());
            match lr.kind {
                LoadKind::Initial => {}
                LoadKind::Reload => match &lr.source {
                    // `R` exists to be current, so a reload that could only
                    // rebuild from the cache must not claim it reloaded.
                    app::DatasetSource::Fallback { failure, .. } => app.set_status(
                        format!(
                            "Reloaded from cache: GitHub {}",
                            ui::github_failure_label(failure)
                        ),
                        true,
                    ),
                    _ => app.set_status("Reloaded".to_string(), false),
                },
                // `R` under a known limit: nothing was sent, so the status
                // says when GitHub resumes instead of `Reloaded` (A6).
                LoadKind::FromMemory => {
                    let status = match github_limit_until(app, config, now) {
                        Some(until) => format!("Rebuilt locally: {}", limited_status(until)),
                        None => "Rebuilt locally".to_string(),
                    };
                    app.set_status(status, false);
                }
                LoadKind::WindowApply(w) => {
                    window_store.save(&w);
                    let label = app.header.clone();
                    app.set_status(format!("Window: {label}"), false);
                }
            }
            // After the load's own message, so it wins (N2): the clone stays
            // unclaimed on every load until it gets an origin remote, and a
            // `Reloaded` that hid the reason would leave it unexplained.
            if let Some(line) = lr.ambiguity {
                app.set_status(line, true);
            }
        }
        Err(e) => {
            if let Some(failure) = e.github_failure {
                // An applied attempt: its reading first, then its outcome.
                app.observe_budget(e.budget.as_deref(), true, now);
                app.github_attempt_failed(&failure, now);
                app.github_freshness.failed(failure, now);
            }
            // The status message says it once; `load_error` keeps saying it
            // for as long as there is nothing on screen to say it instead.
            app.set_status(format!("Load failed: {}", e.message), true);
            app.load_error = Some(e.message);
        }
    }
}

/// What a landed load tells the budget and the retry ladder. A load that went
/// to GitHub is an applied attempt (it supersedes every poll out), so its
/// reading applies and its outcome counts; a cache read and a load with no
/// fetch behind it tell them nothing.
fn record_load_attempt(
    app: &mut App,
    source: &app::DatasetSource,
    reading: Option<&github::BudgetReading>,
    now: chrono::DateTime<chrono::Local>,
) {
    match source {
        app::DatasetSource::Local | app::DatasetSource::Cache { .. } => {}
        app::DatasetSource::Network => {
            app.observe_budget(reading, true, now);
            app.github_backoff.succeeded();
        }
        app::DatasetSource::Fallback { failure, .. } => {
            app.observe_budget(reading, true, now);
            app.github_attempt_failed(failure, now);
        }
    }
}

/// Run the fetch cap's pre-check on every row against `dataset`
/// (`FetchFacts::assess_growth`): whenever the dataset or the rows change,
/// so every automatic selection reads a current hold off the row.
pub(crate) fn assess_size_holds(
    repos: &mut [RepoStatus],
    dataset: &[github::GithubRepo],
    cap: Option<u64>,
) {
    for repo in repos {
        let current = disk_usage_of(repo, dataset);
        repo.fetch.assess_growth(current, cap);
    }
}

/// Rows a load rebuilt, with what this session learned about each checkout's
/// fetches carried over from the row it replaces (`FetchFacts::carried_into`).
/// Matched by path, the key the fetch facts are kept under.
fn carry_fetch_facts(old: &[RepoStatus], mut loaded: Vec<RepoStatus>) -> Vec<RepoStatus> {
    let by_path: HashMap<&str, &model::FetchFacts> = old
        .iter()
        .filter_map(|r| Some((r.path.as_deref()?, &r.fetch)))
        .collect();
    for repo in &mut loaded {
        if let Some(kept) = repo.path.as_deref().and_then(|p| by_path.get(p)) {
            repo.fetch = kept.carried_into(&repo.fetch);
        }
    }
    loaded
}

#[cfg(test)]
mod tests;
