//! Fetches under a claim: one fetch per checkout at a time, with throttled
//! progress.

use super::Sink;
use crate::git;
use crate::model::RepoStatus;
use std::collections::HashSet;
use std::path::Path;

/// At most one progress event per `PROGRESS_EVENT_GAP` per fetch:
/// git prints an update on every percent and every second, more than a row
/// can show.
pub(crate) const PROGRESS_EVENT_GAP: std::time::Duration = std::time::Duration::from_millis(500);

/// Decides which of one fetch's progress updates go to the app.
#[derive(Debug, Default)]
pub(super) struct ProgressThrottle {
    last_sent: Option<std::time::Instant>,
}

impl ProgressThrottle {
    /// Whether an update at `now` may be sent; it is counted as sent if so.
    pub(super) fn admit(&mut self, now: std::time::Instant) -> bool {
        let due = self
            .last_sent
            .is_none_or(|last| now.saturating_duration_since(last) >= PROGRESS_EVENT_GAP);
        if due {
            self.last_sent = Some(now);
        }
        due
    }
}

/// Run one fetch under its claim (`FetchClaims`): taken at the moment
/// the caller's fetch permit was granted, before git starts, and kept alive
/// by each progress update. `NotAttempted` without running git when a newer
/// claim holds the path. With `tx`, progress goes to the row, throttled.
#[cfg(test)]
pub(super) async fn fetch_under_claim(
    claims: &FetchClaims,
    display_path: &str,
    fs_path: &Path,
    token: u64,
    tx: Option<&(dyn Sink<(String, git::FetchProgress)> + Sync)>,
) -> git::FetchOutcome {
    fetch_under_claim_capped(claims, display_path, fs_path, token, tx, None).await
}

/// `fetch_under_claim`, cut as `TooBig` past `max_bytes` received (FZ-35b).
pub(crate) async fn fetch_under_claim_capped(
    claims: &FetchClaims,
    display_path: &str,
    fs_path: &Path,
    token: u64,
    tx: Option<&(dyn Sink<(String, git::FetchProgress)> + Sync)>,
    max_bytes: Option<u64>,
) -> git::FetchOutcome {
    if !claims.start(display_path, token, std::time::Instant::now()) {
        return git::FetchOutcome::NotAttempted;
    }
    let mut throttle = ProgressThrottle::default();
    git::fetch_reporting(fs_path, max_bytes, |progress| {
        let now = std::time::Instant::now();
        claims.touch(display_path, token, now);
        if let Some(tx) = tx
            && throttle.admit(now)
        {
            tx.send_event((display_path.to_string(), progress));
        }
    })
    .await
}

/// One fetch's hold on its path: which claim it is, and when it last showed
/// life (claimed, granted its permit, or reported progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FetchClaim {
    token: u64,
    at: std::time::Instant,
}

#[derive(Debug, Default)]
struct ClaimTable {
    next_token: u64,
    claims: std::collections::HashMap<String, FetchClaim>,
}

/// The paths with a fetch running or queued, one fetch per checkout at a
/// time (FZ-34 R5, FZ-35a). Shared between `App` and the fetch tasks, so a
/// fetch can take its claim at the moment it may start.
///
/// The life of a claim:
///
/// - **Reserved when the sweep is scheduled** (`reserve`, through
///   `App::claim_fetch`), so the next selection already sweeps the path
///   without fetching it while the fetch waits for a permit.
/// - **Taken when the fetch permit is granted** (`start`), in the fetch task,
///   before git runs. This restamps the claim, so time spent queued behind
///   `FETCH_CONCURRENCY` other fetches never counts toward its expiry. A
///   reservation that expired while queued is taken again if the path is
///   free; if a newer claim holds it, this fetch does not run at all. The
///   load path (`assemble_view`) takes its claims here too, with no
///   reservation, since it schedules and starts in one place.
/// - **Kept alive by every progress update** (`touch`).
/// - **Ended by its own landing** (`release` with its token): a late landing
///   from a claim that expired cannot end a newer claim on the same path.
/// - **Expired** after `SWEEP_STUCK_AFTER` without a sign of life, the
///   safety net for a result that never lands (`expire`).
///
/// Each claim carries a token from a counter, never reused. Cloning shares
/// the table.
#[derive(Debug, Clone, Default)]
pub struct FetchClaims {
    table: std::sync::Arc<std::sync::Mutex<ClaimTable>>,
}

impl FetchClaims {
    fn table(&self) -> std::sync::MutexGuard<'_, ClaimTable> {
        // A panic while holding the lock leaves the table consistent: every
        // operation is a single map edit.
        self.table
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reserve `path` at `now`, or `None` when another claim holds it.
    pub fn reserve(&self, path: &str, now: std::time::Instant) -> Option<u64> {
        let mut table = self.table();
        if table.claims.contains_key(path) {
            return None;
        }
        table.next_token += 1;
        let token = table.next_token;
        table
            .claims
            .insert(path.to_string(), FetchClaim { token, at: now });
        Some(token)
    }

    /// The fetch holding `token` was granted its permit at `now`: true when
    /// it may run. Its own claim is restamped; a free path is claimed again
    /// under the same token; a path another claim holds is refused.
    pub fn start(&self, path: &str, token: u64, now: std::time::Instant) -> bool {
        let mut table = self.table();
        match table.claims.get_mut(path) {
            Some(claim) if claim.token == token => claim.at = now,
            Some(_) => return false,
            None => {
                table
                    .claims
                    .insert(path.to_string(), FetchClaim { token, at: now });
            }
        }
        true
    }

    /// A progress update from the fetch holding `token`.
    pub fn touch(&self, path: &str, token: u64, now: std::time::Instant) {
        if let Some(claim) = self.table().claims.get_mut(path)
            && claim.token == token
        {
            claim.at = now;
        }
    }

    /// End the claim on `path`: only the one holding `token`, or whichever
    /// holds it when `token` is `None`.
    pub fn release(&self, path: &str, token: Option<u64>) {
        let mut table = self.table();
        if token.is_none_or(|t| table.claims.get(path).is_some_and(|c| c.token == t)) {
            table.claims.remove(path);
        }
    }

    /// The token of the claim on `path`.
    pub fn token(&self, path: &str) -> Option<u64> {
        self.table().claims.get(path).map(|c| c.token)
    }

    /// Drop claims with no sign of life for longer than `limit` at `now`.
    pub fn expire(&self, limit: std::time::Duration, now: std::time::Instant) {
        self.table()
            .claims
            .retain(|_, claim| now.saturating_duration_since(claim.at) <= limit);
    }

    pub fn contains_key(&self, path: &str) -> bool {
        self.table().claims.contains_key(path)
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.table().claims.is_empty()
    }

    #[cfg(test)]
    pub fn clear(&self) {
        self.table().claims.clear();
    }
}

/// What a batch of fetches someone asked for came to, once the last landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchReport {
    pub total: usize,
    /// Repo name and failure kind of each fetch that failed, in landing order.
    pub failed: Vec<(String, crate::git::FetchFailureKind)>,
}

/// The fetches a keypress is waiting on, so a mixed result is reported as
/// mixed (rule 9 of the fetch contract): `3 of 12 fetches failed`, not
/// silence. Only fetches someone asked for are tallied; a watch sweep's
/// failures show on their rows and nowhere else.
///
/// Pure transitions, like `GithubFreshness`: `expect` when fetches start,
/// `land` as each one finishes, which hands back the report with the last.
#[derive(Debug, Default)]
pub struct FetchTally {
    pub(crate) awaiting: std::collections::HashSet<String>,
    total: usize,
    failed: Vec<(String, crate::git::FetchFailureKind)>,
    /// The batch's last sign of life: when it started, or the latest
    /// progress of a fetch it awaits (`touch`). What `expire` measures from.
    pub(crate) started: Option<std::time::Instant>,
}

impl FetchTally {
    /// Fetches for these display paths have started. Joins a batch still in
    /// flight, so `r` and `F` pressed together report once.
    pub fn expect(&mut self, paths: impl IntoIterator<Item = String>) {
        if self.awaiting.is_empty() {
            *self = Self::default();
        }
        for path in paths {
            if self.awaiting.insert(path) {
                self.total += 1;
            }
        }
        if !self.awaiting.is_empty() && self.started.is_none() {
            self.started = Some(std::time::Instant::now());
        }
    }

    /// A repo's update landed. An update that did not fetch is not the one
    /// awaited. Returns the report when this was the last fetch awaited and
    /// at least one failed; every fetch succeeding says nothing extra.
    pub fn land(
        &mut self,
        path: &str,
        name: &str,
        fetch: &crate::git::FetchOutcome,
    ) -> Option<FetchReport> {
        use crate::git::FetchOutcome;
        if *fetch == FetchOutcome::NotAttempted || !self.awaiting.remove(path) {
            return None;
        }
        if let FetchOutcome::Failed(failure) = fetch {
            self.failed.push((name.to_string(), failure.kind));
        }
        if !self.awaiting.is_empty() {
            return None;
        }
        let done = std::mem::take(self);
        (!done.failed.is_empty()).then_some(FetchReport {
            total: done.total,
            failed: done.failed,
        })
    }

    /// A fetch for `path` made progress at `now`: if this batch awaits it,
    /// the batch is alive. An uncapped `f` on a big repo can run far longer
    /// than `SWEEP_STUCK_AFTER` and still owes its report (FZ-35b,
    /// criterion 7).
    pub fn touch(&mut self, path: &str, now: std::time::Instant) {
        if self.awaiting.contains(path) {
            self.started = Some(now);
        }
    }

    /// Stop waiting on fetches that have shown no sign of life for longer
    /// than `limit`. They report nothing; their rows say what happened if
    /// they ever land.
    pub fn expire(&mut self, limit: std::time::Duration) {
        if self.started.is_some_and(|t| t.elapsed() > limit) {
            *self = Self::default();
        }
    }
}

/// One repo queued for a sweep.
pub(crate) struct SweepTarget {
    /// Display path as stored in RepoStatus.path (tilde form), the update key.
    pub(crate) display_path: String,
    pub(crate) fs_path: std::path::PathBuf,
    /// The row as it was when the sweep started: the WorkKey baseline and
    /// the source of reused probes. Its non-local fields are stale by the
    /// time the result lands, which is why `apply_local_sweep` ignores them.
    pub(crate) old: Box<RepoStatus>,
}

/// Who asked for a fetch, which decides the holds it honours and whether
/// the fetch cap bounds it (FZ-34, FZ-35b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FetchAsk {
    /// The watch timer, a moved remote, the recovery refetch: skips a
    /// timeout hold and a size hold, and is capped.
    Automatic,
    /// `F`: fetches through a timeout hold, but skips a size hold and is
    /// capped. Nothing but a deliberate per-row key pulls a big repo.
    All,
    /// `f` on one row: the permission. Fetches through both holds, uncapped.
    One,
}

impl FetchAsk {
    /// A full sweep: `F` when `manual`, else the watch timer.
    pub(crate) fn for_full_sweep(manual: bool) -> Self {
        if manual {
            FetchAsk::All
        } else {
            FetchAsk::Automatic
        }
    }

    /// A single-repo sweep: a moved remote or the recovery refetch when
    /// `automatic`, else `f` (a single sweep that does not fetch is no ask).
    pub(crate) fn for_single_sweep(automatic: bool) -> Self {
        if automatic {
            FetchAsk::Automatic
        } else {
            FetchAsk::One
        }
    }

    /// The byte cap this fetch runs under, given the configured one.
    pub(crate) fn cap(self, configured: Option<u64>) -> Option<u64> {
        match self {
            FetchAsk::One => None,
            FetchAsk::Automatic | FetchAsk::All => configured,
        }
    }
}

/// The display paths of the rows the fetch cap holds until `f` (FZ-35b).
pub(crate) fn size_held_rows(repos: &[RepoStatus]) -> HashSet<String> {
    repos
        .iter()
        .filter(|r| r.fetch.size_hold().is_some())
        .filter_map(|r| r.path.clone())
        .collect()
}
