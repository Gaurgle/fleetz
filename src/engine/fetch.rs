//! Fetches under a claim: one fetch per checkout at a time, with throttled
//! progress; and `FetchState`, the local fetch concern the TUI holds: the
//! sweep lock, the refresh flags, the timeout holds, the claims and the
//! tally (E2b in `docs/app-split-design.md`).

use super::Sink;
use super::backoff_secs;
use crate::git;
use crate::model::RepoStatus;
use chrono::{DateTime, Local};
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
/// time (FZ-34 R5, FZ-35a). Shared between `FetchState` and the fetch tasks,
/// so a fetch can take its claim at the moment it may start.
///
/// The life of a claim:
///
/// - **Reserved when the sweep is scheduled** (`reserve`, through
///   `FetchState::claim`), so the next selection already sweeps the path
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

/// The two halves of a manual `r`, named apart in the header because they
/// cost very different amounts: the local sweep lands in well under a second,
/// the GitHub poll takes 6-7s before its fetches even start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPhase {
    /// Re-reading the local clones.
    Local,
    /// Waiting on the GitHub poll, then on the fetches it set off.
    Remote,
}

/// The per-checkout hold after fetches that timed out (FZ-34, #83; amendment
/// 2 of `docs/fetch-outcome-contract.md`). A repo too large to fetch within
/// FZ-34's wall clock used to start the same download on every sweep and
/// leave a temp pack behind each time; since FZ-35a a timeout is a fetch
/// that stalled for `git::FETCH_STALL_LIMIT`.
///
/// Keyed by the row's path, as the fetch facts are. After the `n`th
/// consecutive `TimedOut` fetch the path is held for
/// `backoff_secs(n + 1, interval_secs, 1.0)` seconds from that timeout: two
/// intervals, then four, doubling up to `BACKOFF_CAP_SECS`, with no jitter
/// (one clone has nothing to desynchronise from). Only the automatic fetches
/// honour it (the watch sweep, the poll's moved remotes, the recovery
/// refetch); `f` and `F` fetch as before and their outcome counts like any.
/// A success clears the path; `r` and `R` clear every path. Any other failure
/// kind fails fast, costs nothing, and leaves the count as it was. That
/// includes `TooBig` (FZ-35b): a fetch cut at the cap holds its repo
/// explicitly, until `f` or a success (`model::FetchFacts::size_hold`), not
/// on a doubling timer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchBackoff {
    /// Consecutive timeouts and when the last one landed, per path.
    timeouts: std::collections::HashMap<String, (u32, DateTime<Local>)>,
}

impl FetchBackoff {
    /// A fetch for `path` finished at `at`. A timeout older than the one
    /// already counted (a load's fetch landing after a sweep's) is ignored,
    /// so one timeout is never counted twice.
    pub fn record(&mut self, path: &str, outcome: &crate::git::FetchOutcome, at: DateTime<Local>) {
        use crate::git::{FetchFailureKind, FetchOutcome};
        match outcome {
            FetchOutcome::Succeeded => {
                self.timeouts.remove(path);
            }
            FetchOutcome::Failed(f) if f.kind == FetchFailureKind::TimedOut => {
                let entry = self.timeouts.entry(path.to_string()).or_insert((0, at));
                if entry.0 == 0 || at > entry.1 {
                    *entry = (entry.0.saturating_add(1), at);
                }
            }
            FetchOutcome::Failed(_) | FetchOutcome::NotAttempted => {}
        }
    }

    /// Whether an automatic fetch of `path` must wait at `now`.
    pub fn holds(&self, path: &str, now: DateTime<Local>, interval_secs: i64) -> bool {
        self.timeouts.get(path).is_some_and(|&(n, at)| {
            now < at + chrono::Duration::seconds(backoff_secs(n + 1, interval_secs, 1.0))
        })
    }

    /// A manual `r` or `R`: every path may fetch again.
    pub fn clear(&mut self) {
        self.timeouts.clear();
    }

    /// The consecutive timeouts counted for `path`.
    #[cfg(test)]
    pub fn timeouts(&self, path: &str) -> u32 {
        self.timeouts.get(path).map_or(0, |&(n, _)| n)
    }
}

/// The local fetch concern: the sweep lock, the manual refresh's two
/// halves, the timeout holds, the fetch claims and the keypress tally, kept
/// together because they must agree (a claim and the tally awaiting it, a
/// hold and its ladder). `App` holds one; it decides and never sends an
/// event (D6): the caller spawns what a verb returns.
pub struct FetchState {
    /// When the in-flight watch sweep started, or None when idle. Doubles as
    /// the "one sweep at a time" lock and as the stuck-sweep timer.
    sweep_started: Option<std::time::Instant>,
    /// Whether the in-flight sweep was asked for by a keypress rather than by
    /// the watch timer. Only a manual one announces itself in the header: a
    /// background refresh firing on an interval should not repaint a big
    /// colored indicator over a dashboard you are reading. An `r` pressed
    /// while a watch sweep runs marks that sweep manual: it starts none of
    /// its own, so the running one's rows are the ones it waits for.
    ///
    /// Only meaningful while `sweep_started` is `Some`.
    sweep_manual: bool,
    /// True while a manual `r` waits on its GitHub poll. The local sweep is
    /// only half of a refresh: the poll lands later and fetches the remotes
    /// that moved, so a spinner keyed on the sweep alone stopped while rows
    /// were still changing under it.
    refresh_awaits_poll: bool,
    /// Display paths of the fetches that poll set off for a manual `r`, each
    /// cleared by that repo's fetched update. See `is_refreshing`.
    refresh_fetches: HashSet<String>,
    /// When `refresh_fetches` was filled, or one of its fetches last made
    /// progress (`touch`), so a fetch that never returns cannot leave the
    /// spinner running for the rest of the session while one still
    /// downloading is waited for.
    refresh_fetches_started: Option<std::time::Instant>,
    /// Paths the automatic fetches skip after a timeout (`FetchBackoff`).
    fetch_backoff: FetchBackoff,
    /// The automatic-fetch cap in bytes, `None` for no cap (`fetch_max_mb`,
    /// FZ-35b). Copied from `Config` at startup, as the poll interval is.
    fetch_max_bytes: Option<u64>,
    /// Paths with a fetch running or queued: one fetch per repo at a time
    /// (FZ-34, R5). Every fetch selection, automatic or manual, skips a path
    /// listed here and only sweeps it (`claim`). Two fetches of one repo
    /// each wrote their own temp pack. Sweeps reserve here and the fetch
    /// tasks take the claim at their permit; a launch or `R` load claims its
    /// fetches here too (`FetchClaims`, FZ-35a).
    fetches_in_flight: FetchClaims,
    /// The fetches a keypress is waiting on (`f`, `F`, and the ones `r`'s
    /// poll sets off), reported as one status line when the last lands.
    manual_fetches: FetchTally,
}

/// What a sweep hands `spawn_sweep` about fetching: the targets it rebuilds
/// without a fetch, and the byte cap the rest run under (FZ-35b).
pub struct SweepFetchPlan {
    pub(crate) no_fetch: HashSet<String>,
    pub(crate) max_bytes: Option<u64>,
}

impl FetchState {
    /// No sweep, no refresh, no hold, no claim, under the cap `max_bytes`.
    pub fn new(max_bytes: Option<u64>) -> Self {
        Self {
            sweep_started: None,
            sweep_manual: false,
            refresh_awaits_poll: false,
            refresh_fetches: HashSet::new(),
            refresh_fetches_started: None,
            fetch_backoff: FetchBackoff::default(),
            fetch_max_bytes: max_bytes,
            fetches_in_flight: FetchClaims::default(),
            manual_fetches: FetchTally::default(),
        }
    }

    /// Take the sweep lock at `now` for a sweep asked for by keypress when
    /// `manual`. False, changing nothing, while a sweep already holds it.
    pub fn begin_sweep(&mut self, now: std::time::Instant, manual: bool) -> bool {
        if self.sweep_started.is_some() {
            return false;
        }
        self.sweep_started = Some(now);
        self.sweep_manual = manual;
        true
    }

    /// The sweep finished: release the lock.
    pub fn sweep_landed(&mut self) {
        self.sweep_started = None;
    }

    /// A sweep that never reported back must not hold the lock forever:
    /// release it once it is older than `limit`.
    pub fn expire_sweep(&mut self, limit: std::time::Duration) {
        if let Some(started) = self.sweep_started
            && started.elapsed() > limit
        {
            self.sweep_started = None;
        }
    }

    /// Whether a sweep holds the lock.
    pub fn in_sweep(&self) -> bool {
        self.sweep_started.is_some()
    }

    /// An `r` pressed while a watch sweep runs: that sweep becomes the
    /// refresh's local half (FZ-85). Nothing when no sweep runs.
    pub fn mark_manual(&mut self) {
        if self.sweep_started.is_some() {
            self.sweep_manual = true;
        }
    }

    /// The fetch half of a sweep asked for by `ask`: the targets it rebuilds
    /// without fetching, claiming the rest for a fetch at `instant` (FZ-34),
    /// and the cap those run under (`FetchAsk::cap`). Every fetching sweep
    /// skips a path whose fetch is still running (R5); an automatic one (the
    /// watch timer, a moved remote, the recovery refetch) also skips a path
    /// held after a timeout at `now` (R3, measured in `interval_secs`), where
    /// `f` and `F` fetch it as asked. Every one but `f` skips the targets the
    /// fetch cap holds, `size_held` (FZ-35b). A sweep that does not fetch
    /// claims nothing.
    #[allow(clippy::too_many_arguments)] // the inputs App gathers (D2); a params struct would only rename them
    pub fn plan(
        &mut self,
        targets: &[SweepTarget],
        do_fetch: bool,
        ask: FetchAsk,
        size_held: &HashSet<String>,
        interval_secs: i64,
        now: DateTime<Local>,
        instant: std::time::Instant,
    ) -> SweepFetchPlan {
        let no_fetch = if !do_fetch {
            HashSet::new()
        } else {
            let mut skips = if ask == FetchAsk::Automatic {
                self.held_paths(targets, now, interval_secs)
            } else {
                HashSet::new()
            };
            if ask != FetchAsk::One {
                skips.extend(size_held.iter().cloned());
            }
            for target in targets {
                if !skips.contains(&target.display_path)
                    && !self.claim(&target.display_path, instant)
                {
                    skips.insert(target.display_path.clone());
                }
            }
            skips
        };
        SweepFetchPlan {
            no_fetch,
            max_bytes: ask.cap(self.fetch_max_bytes),
        }
    }

    /// The targets an automatic sweep rebuilds without fetching at `now`:
    /// those a recent fetch timeout holds (`held`, FZ-34).
    pub(crate) fn held_paths(
        &self,
        targets: &[SweepTarget],
        now: DateTime<Local>,
        interval_secs: i64,
    ) -> HashSet<String> {
        targets
            .iter()
            .filter(|t| self.held(&t.display_path, now, interval_secs))
            .map(|t| t.display_path.clone())
            .collect()
    }

    /// Fetches for these display paths started for a keypress: report them
    /// when the last lands (`FetchTally::expect`).
    pub fn expect_manual(&mut self, paths: impl IntoIterator<Item = String>) {
        self.manual_fetches.expect(paths);
    }

    /// A repo's update landed: the keypress report, when this was the last
    /// fetch it awaited and one failed (`FetchTally::land`).
    pub fn land_manual(
        &mut self,
        path: &str,
        name: &str,
        fetch: &crate::git::FetchOutcome,
    ) -> Option<FetchReport> {
        self.manual_fetches.land(path, name, fetch)
    }

    /// Whether a manual `r` waits on a GitHub poll: set when it starts one
    /// or one is already in flight, cleared when that poll is given up on.
    pub fn await_poll(&mut self, awaiting: bool) {
        self.refresh_awaits_poll = awaiting;
    }

    /// Which half of a manual `r` is running, or None when none is: first its
    /// local sweep, then the GitHub poll it started and the fetches that poll
    /// set off. The spinner and the lit `[r]` key both read this, so neither
    /// stops before the last row lands. A watch-timer sweep does not count; it
    /// runs silently, unless an `r` pressed during it marked it manual.
    pub fn refresh_phase(&self) -> Option<RefreshPhase> {
        if self.sweep_started.is_some() && self.sweep_manual {
            Some(RefreshPhase::Local)
        } else if self.refresh_awaits_poll || !self.refresh_fetches.is_empty() {
            Some(RefreshPhase::Remote)
        } else {
            None
        }
    }

    /// Whether a manual `r` is still in flight, in either half.
    pub fn is_refreshing(&self) -> bool {
        self.refresh_phase().is_some()
    }

    /// Hand the fetches a GitHub poll set off to the manual refresh waiting
    /// on that poll, if there is one. A poll nobody pressed `r` for (the
    /// watch timer) fetches silently and adds nothing.
    pub fn settle_refresh_poll(&mut self, fetched_paths: impl IntoIterator<Item = String>) {
        if !std::mem::take(&mut self.refresh_awaits_poll) {
            return;
        }
        let fetched_paths: Vec<String> = fetched_paths.into_iter().collect();
        self.manual_fetches.expect(fetched_paths.iter().cloned());
        self.refresh_fetches.extend(fetched_paths);
        if !self.refresh_fetches.is_empty() {
            self.refresh_fetches_started = Some(std::time::Instant::now());
        }
    }

    /// A repo's update landed. Only one whose fetch finished settles a
    /// pending refresh fetch, whether it succeeded or failed: the local sweep
    /// can report the same repo before its fetch has run, and that update does
    /// not carry the remote's news yet.
    pub fn settle_refresh_fetch(&mut self, path: &str, fetch: &crate::git::FetchOutcome) {
        let finished = *fetch != crate::git::FetchOutcome::NotAttempted;
        if finished && self.refresh_fetches.remove(path) && self.refresh_fetches.is_empty() {
            self.refresh_fetches_started = None;
        }
    }

    /// Whether an automatic fetch (watch sweep, moved remote, recovery
    /// refetch) skips `path` at `now`: a recent timeout holds it, measured in
    /// watch intervals of `interval_secs` (`FetchBackoff`).
    pub fn held(&self, path: &str, now: DateTime<Local>, interval_secs: i64) -> bool {
        self.fetch_backoff.holds(path, now, interval_secs)
    }

    /// Claim `path` for a fetch starting at `now`. False when a fetch of it is
    /// already running: the caller sweeps the repo without fetching, and the
    /// running fetch's outcome is the one that lands (R5).
    pub fn claim(&mut self, path: &str, now: std::time::Instant) -> bool {
        self.fetches_in_flight.reserve(path, now).is_some()
    }

    /// `landed_under` for whichever claim holds `path`.
    #[cfg(test)]
    pub fn landed(&mut self, path: &str, fetch: &crate::git::FetchOutcome) {
        self.landed_under(path, fetch, None);
    }

    /// A sweep's result for `path` landed, from the fetch that held `claim`.
    /// Only one that fetched ends the claim: a sweep that skipped the fetch
    /// reports `NotAttempted` while the claimed fetch is still running. It
    /// ends that claim only, so a landing that outlived its claim leaves a
    /// newer one alone (`FetchClaims::release`). `None` ends whichever claim
    /// holds the path.
    pub fn landed_under(
        &mut self,
        path: &str,
        fetch: &crate::git::FetchOutcome,
        claim: Option<u64>,
    ) {
        if *fetch != crate::git::FetchOutcome::NotAttempted {
            self.fetches_in_flight.release(path, claim);
        }
    }

    /// Drop claims with no sign of life for longer than `limit` at `now`, the
    /// safety net for a fetch whose result never landed
    /// (`SWEEP_STUCK_AFTER`).
    pub fn expire_claims(&mut self, limit: std::time::Duration, now: std::time::Instant) {
        self.fetches_in_flight.expire(limit, now);
    }

    /// A progress update at `now` for the fetch running on `path`: a sign of
    /// life for a refresh or a fetch report waiting on that fetch, which then
    /// expire from their last update rather than from their start, as the
    /// fetch's claim does (FZ-35b, criterion 7).
    pub fn touch(&mut self, path: &str, now: std::time::Instant) {
        self.manual_fetches.touch(path, now);
        if self.refresh_fetches.contains(path) {
            self.refresh_fetches_started = Some(now);
        }
    }

    /// Give up on refresh fetches with no sign of life for longer than
    /// `limit`, and on a fetch report silent for as long.
    pub fn expire_refresh(&mut self, limit: std::time::Duration) {
        if self
            .refresh_fetches_started
            .is_some_and(|t| t.elapsed() > limit)
        {
            self.refresh_fetches.clear();
            self.refresh_fetches_started = None;
        }
        self.manual_fetches.expire(limit);
    }

    /// A fetch for `path` finished at `at`: count it toward the timeout hold
    /// (`FetchBackoff::record`).
    pub fn record_outcome(
        &mut self,
        path: &str,
        outcome: &crate::git::FetchOutcome,
        at: DateTime<Local>,
    ) {
        self.fetch_backoff.record(path, outcome, at);
    }

    /// A manual `r` or `R`: lift every timeout hold (FZ-34).
    pub fn clear_holds(&mut self) {
        self.fetch_backoff.clear();
    }

    /// The configured automatic-fetch cap, set from `Config` at startup.
    pub fn set_max_bytes(&mut self, cap: Option<u64>) {
        self.fetch_max_bytes = cap;
    }

    /// The automatic-fetch cap in bytes, `None` for no cap.
    pub fn max_bytes(&self) -> Option<u64> {
        self.fetch_max_bytes
    }

    /// The claim table, shared with the fetch tasks (`FetchClaims`).
    pub fn claims(&self) -> &FetchClaims {
        &self.fetches_in_flight
    }
}

/// Where tests poke the state they used to write on `App` directly (D7).
#[cfg(test)]
impl FetchState {
    pub fn set_sweep_started(&mut self, started: Option<std::time::Instant>) {
        self.sweep_started = started;
    }

    pub fn set_sweep_manual(&mut self, manual: bool) {
        self.sweep_manual = manual;
    }

    pub fn awaits_poll(&self) -> bool {
        self.refresh_awaits_poll
    }

    pub fn refresh_fetches(&self) -> &HashSet<String> {
        &self.refresh_fetches
    }

    pub fn refresh_fetches_mut(&mut self) -> &mut HashSet<String> {
        &mut self.refresh_fetches
    }

    pub fn set_refresh_fetches_started(&mut self, started: Option<std::time::Instant>) {
        self.refresh_fetches_started = started;
    }

    pub fn refresh_fetches_started(&self) -> Option<std::time::Instant> {
        self.refresh_fetches_started
    }

    pub fn tally(&self) -> &FetchTally {
        &self.manual_fetches
    }

    pub fn set_tally_started(&mut self, started: Option<std::time::Instant>) {
        self.manual_fetches.started = started;
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

#[cfg(test)]
mod tests;
