//! A whole load: the GitHub dataset (network or cache) and the assembly over
//! it, or the local-only rows of dirty mode.

use super::assembly::{ViewNotes, assemble_view, probe_concurrency, resolve_cutoff};
use super::fetch::FetchClaims;
use super::status::dirty_mode_status;
use crate::cli::{Args, Mode};
use crate::model::RepoStatus;
use crate::{cache, config, contributions, discovery, github, time};
use chrono::{DateTime, Local};
use std::collections::HashSet;
use std::path::Path;

/// Why a load produced nothing to show.
#[derive(Debug)]
pub struct LoadError {
    pub message: String,
    /// The GitHub attempt behind it, when that is what failed. A failed
    /// attempt still advances the retry clock even though nothing landed.
    pub github_failure: Option<github::Failure>,
    /// The budget reading the failed attempt's answer carried, if any, so a
    /// launch refused by a rate limit learns its reset.
    /// Boxed so the error stays small enough to return by value (clippy's
    /// `result_large_err`), as `FetchError.meta` is.
    pub budget: Option<Box<github::BudgetReading>>,
}

impl From<String> for LoadError {
    fn from(message: String) -> Self {
        Self {
            message,
            github_failure: None,
            budget: None,
        }
    }
}

/// Load all repo data: GitHub API + local discovery + git status.
/// Returns the assembled view plus the unfiltered dataset so the TUI can
/// re-filter it live (time-window overlay) without re-fetching.
/// `size_held` are the display paths the fetch cap holds, which its fetches
/// skip (FZ-35b).
#[allow(clippy::too_many_arguments)] // one call spine; a params struct would only rename the problem
pub(crate) async fn load_repos(
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    mode: &Mode,
    window: Option<time::TimeWindow>,
    bypass_cache: bool,
    fetch_claims: &FetchClaims,
    size_held: &HashSet<String>,
) -> Result<
    (
        Vec<RepoStatus>,
        ViewNotes,
        Vec<github::GithubRepo>,
        Option<Box<contributions::Calendar>>,
        DatasetSource,
        Option<github::FetchMeta>,
    ),
    LoadError,
> {
    // Local scan roots: the primary repo_dir plus any configured extra dirs
    // (e.g. ~/.dotfiles), so repos outside the main tree still surface under
    // "local changes" -- the fleetz equivalent of repoz's EXTRA_DIRS.
    let mut scan_roots = vec![repo_dir.to_path_buf()];
    scan_roots.extend(config.resolved_extra_dirs());

    // Dirty mode: local only, no GitHub
    if *mode == Mode::Dirty {
        let header = "local changes".to_string();
        let local_paths = discovery::find_all_local(&scan_roots);

        // Probe every candidate in parallel behind a semaphore, then put the
        // survivors back in discovery order.
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(probe_concurrency()));
        let mut set = tokio::task::JoinSet::new();
        for (i, path) in local_paths.into_iter().enumerate() {
            let sem = sem.clone();
            let author = args.author.clone();
            set.spawn(async move {
                let _permit = sem.acquire().await;
                (i, dirty_mode_status(&path, author.as_deref()).await)
            });
        }
        let mut results: Vec<(usize, Option<RepoStatus>)> = Vec::new();
        while let Some(joined) = set.join_next().await {
            if let Ok(pair) = joined {
                results.push(pair);
            }
        }
        results.sort_by_key(|(i, _)| *i);
        let repos: Vec<RepoStatus> = results.into_iter().filter_map(|(_, r)| r).collect();
        let notes = ViewNotes {
            header,
            ambiguity: None,
        };
        return Ok((repos, notes, Vec::new(), None, DatasetSource::Local, None));
    }

    // Fetch from GitHub (with caching)
    let api_cache = cache::KeyedCache::new(config.repo_limit, github::HOST);
    let (fetch, source) = match fetch_dataset(config, &api_cache, bypass_cache).await {
        Ok(fetch) => {
            let source = match fetch.origin {
                github::Origin::Network => DatasetSource::Network,
                github::Origin::Cache { written } => DatasetSource::Cache { written },
            };
            (fetch, source)
        }
        // Launch and `R` have nothing in memory worth keeping over the
        // cache, so they build from it, but as a failure: the header and
        // the status line say the data is not current.
        Err(github::FetchError {
            failure,
            fallback: Some(mut fallback),
            meta,
        }) => {
            // The refused answer's budget reading, not the cache's (which
            // has none): the attempt is applied when the load lands.
            fallback.meta.budget = meta.budget;
            let written = match fallback.origin {
                github::Origin::Cache { written } => written,
                github::Origin::Network => None,
            };
            (*fallback, DatasetSource::Fallback { failure, written })
        }
        Err(github::FetchError {
            failure,
            fallback: None,
            meta,
        }) => {
            return Err(LoadError {
                message: failure.message.clone(),
                github_failure: Some(failure),
                budget: meta.budget.map(Box::new),
            });
        }
    };
    let (dataset, calendar, meta) = (fetch.repos, fetch.calendar, fetch.meta);

    let (cutoff, header) = resolve_cutoff(window, mode, config)?;
    let (repos, notes) = assemble_view(
        &dataset,
        meta.coverage.as_ref(),
        cutoff,
        header,
        args,
        config,
        repo_dir,
        true,
        None,
        fetch_claims,
        size_held,
    )
    .await?;

    Ok((repos, notes, dataset, calendar, source, Some(meta)))
}

/// The GitHub GraphQL fetch (with disk cache), unfiltered: the top
/// repo_limit repos by push date.
async fn fetch_dataset(
    config: &config::Config,
    cache: &cache::KeyedCache,
    bypass_cache: bool,
) -> Result<github::GithubFetch, github::FetchError> {
    let mode = match bypass_cache {
        true => github::CacheMode::Bypass,
        false => github::CacheMode::AnyCached,
    };
    github::fetch_github(config.repo_limit, cache, mode).await
}

/// Where a load's GitHub dataset came from, which decides what it tells
/// `GithubFreshness`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetSource {
    /// No GitHub fetch behind it: a window apply, or `-d`.
    Local,
    /// Read off the disk cache, with no attempt made.
    Cache { written: Option<DateTime<Local>> },
    /// A successful attempt.
    Network,
    /// A failed attempt, with the cache filling in.
    Fallback {
        failure: crate::github::Failure,
        written: Option<DateTime<Local>>,
    },
}
