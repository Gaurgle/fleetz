//! The GitHub sync (E3, `docs/app-split-design.md`): the fleet poll's three
//! freshness facts, the account's budget, the retry ladder, the in-flight
//! lock and the push shortcut, and the decisions read off them. `App` holds
//! one and the loop spawns what it decides (D6); the decisions take their
//! inputs as arguments, never `&App` (D2).

use super::DatasetSource;
use crate::config::Config;
use crate::time::Pace;
use chrono::{DateTime, Local};
use std::time::{Duration, Instant};

/// How old the GitHub data on screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataTime {
    /// Nothing from GitHub has been shown yet.
    NoData,
    /// Data is shown, but when it left GitHub could not be read. It stays
    /// unknown: launch time is never used in its place.
    Unknown,
    /// When the data on screen left GitHub.
    At(DateTime<Local>),
}

/// How the most recent attempt to reach GitHub ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    NotAttempted,
    Success,
    Failed(crate::github::Failure),
}

/// The three GitHub freshness facts of `docs/freshness-contract.md`: when the
/// data on screen left GitHub, when fleetz last tried, and how that went.
///
/// They used to be one timestamp and one flag, `last_github_poll` and
/// `github_poll_failing`, and an offline poll answered from the cache set the
/// one and cleared the other, so offline looked exactly like healthy. Kept
/// apart, each transition below is a pure function of the event and `now`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubFreshness {
    pub data_time: DataTime,
    /// When the most recent attempt finished or was given up on. Drives the
    /// retry clock and the budget's pace, and dates a failure in the header
    /// (`offline at 12:02`, #79); a cache read never sets it.
    pub last_attempt: Option<DateTime<Local>>,
    pub outcome: Outcome,
    /// The id handed to the newest background poll started.
    started: u64,
    /// The id of the newest background poll whose answer was applied.
    applied: u64,
}

impl Default for GithubFreshness {
    fn default() -> Self {
        Self {
            data_time: DataTime::NoData,
            last_attempt: None,
            outcome: Outcome::NotAttempted,
            started: 0,
            applied: 0,
        }
    }
}

impl GithubFreshness {
    /// A load landed. A cache read is not an attempt (rule 1), so it sets the
    /// data time and leaves the retry clock unset: the poll is due at once.
    ///
    /// A load that went to GitHub (`R`, or a launch with no usable cache) is
    /// an attempt that rebuilt every row, so it supersedes every poll started
    /// before it lands: an answer from one of those cannot be ordered against
    /// the load and must not overwrite it (rule 10). A cache read and a load
    /// with no GitHub fetch behind it supersede nothing, since they are older
    /// than any poll still out, which is about to bring newer data.
    pub fn loaded(&mut self, source: &DatasetSource, now: DateTime<Local>) {
        match source {
            DatasetSource::Local => {}
            DatasetSource::Cache { written } => self.data_time = Self::file_time(*written),
            DatasetSource::Network => {
                self.supersede_started();
                self.succeeded(now);
            }
            DatasetSource::Fallback { failure, written } => {
                self.supersede_started();
                self.failed(failure.clone(), now);
                // The rows were rebuilt from the cache, so this is what is on
                // screen now.
                self.data_time = Self::file_time(*written);
            }
        }
    }

    /// An attempt came back with data (rule 7).
    pub fn succeeded(&mut self, now: DateTime<Local>) {
        self.data_time = DataTime::At(now);
        self.last_attempt = Some(now);
        self.outcome = Outcome::Success;
    }

    /// An attempt failed. The data time stays where it was (rule 3); the
    /// retry clock advances (rule 6).
    pub fn failed(&mut self, failure: crate::github::Failure, now: DateTime<Local>) {
        self.last_attempt = Some(now);
        self.outcome = Outcome::Failed(failure);
    }

    /// A background poll outlived `GITHUB_POLL_STUCK_AFTER` (decision D4).
    pub fn gave_up(&mut self, after: std::time::Duration, now: DateTime<Local>) {
        let failure = crate::github::Failure::new(
            crate::github::FailureKind::TimedOut,
            format!("no answer from GitHub after {}s", after.as_secs()),
        );
        self.failed(failure, now);
    }

    /// Hand out the id for a background poll about to start.
    pub fn begin_attempt(&mut self) -> u64 {
        self.started += 1;
        self.started
    }

    /// Whether `attempt` is the newest poll started, the one the in-flight
    /// lock belongs to.
    pub fn is_latest(&self, attempt: u64) -> bool {
        attempt == self.started
    }

    /// Record that `attempt`'s answer is being applied. False when a newer
    /// attempt's answer was applied already: the caller must discard it, or
    /// the data time could go backwards (rule 10).
    pub fn accept_answer(&mut self, attempt: u64) -> bool {
        if attempt <= self.applied {
            return false;
        }
        self.applied = attempt;
        true
    }

    /// Whether one interval of `every_secs` has passed since the last attempt.
    /// No attempt yet means due.
    pub fn retry_due(&self, now: DateTime<Local>, every_secs: i64) -> bool {
        self.last_attempt
            .is_none_or(|t| (now - t).num_seconds() >= every_secs)
    }

    pub fn is_failing(&self) -> bool {
        matches!(self.outcome, Outcome::Failed(_))
    }

    /// Data is on screen and nothing has tried to confirm it yet: the state a
    /// cache-served launch leaves, owed one attempt even with watch off
    /// (rule 8).
    pub fn awaits_first_attempt(&self) -> bool {
        self.outcome == Outcome::NotAttempted && self.data_time != DataTime::NoData
    }

    /// Treat every poll started so far as answered, so their answers are
    /// discarded when they arrive.
    fn supersede_started(&mut self) {
        self.applied = self.started;
    }

    fn file_time(written: Option<DateTime<Local>>) -> DataTime {
        written.map_or(DataTime::Unknown, DataTime::At)
    }
}

/// Where the randomness in the retry policy comes from, injected so tests can
/// plant it. See `docs/api-budget-design.md` 4.3 and 4.4: instances that fail
/// together would otherwise retry together.
pub trait Jitter {
    /// A factor for the backoff ladder from step 2 on, in 0.8 to 1.2.
    fn backoff_factor(&mut self) -> f64;
    /// Whole seconds added to a resume after a reset, in 0 to 30.
    fn resume_secs(&mut self) -> i64;
}

/// The production jitter: a xorshift seeded from the clock and the process id,
/// so two instances started together still draw apart. Not for anything that
/// needs to be unpredictable, only spread.
pub struct ClockJitter {
    state: u64,
}

impl ClockJitter {
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        let seed = nanos ^ (u64::from(std::process::id()) << 32) ^ 0x9E37_79B9_7F4A_7C15;
        Self { state: seed.max(1) }
    }

    /// A draw in 0.0 to 1.0.
    fn unit(&mut self) -> f64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl Default for ClockJitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Jitter for ClockJitter {
    fn backoff_factor(&mut self) -> f64 {
        BACKOFF_JITTER_MIN + (BACKOFF_JITTER_MAX - BACKOFF_JITTER_MIN) * self.unit()
    }

    fn resume_secs(&mut self) -> i64 {
        ((self.unit() * (RESUME_JITTER_MAX_SECS + 1) as f64) as i64).min(RESUME_JITTER_MAX_SECS)
    }
}

/// The share of the hourly limit pacing leaves untouched for `r`, `R`, the
/// board and other clients on the token (A2).
pub const BUDGET_RESERVE_PERCENT: u64 = 10;
/// Where the backoff ladder stops doubling, before its jitter (A3).
pub const BACKOFF_CAP_SECS: i64 = 600;
/// GitHub's documented minimum wait for a secondary limit that names none.
pub const SECONDARY_LIMIT_MIN_SECS: i64 = 60;
const BACKOFF_JITTER_MIN: f64 = 0.8;
const BACKOFF_JITTER_MAX: f64 = 1.2;
const RESUME_JITTER_MAX_SECS: i64 = 30;
/// A rate-limit window lasts at most an hour, which bounds a reset's delay.
const RESET_WINDOW_SECS: i64 = 3600;

/// The newest budget reading, anchored to the local clock on arrival. Memory
/// only: a budget is per account and per moment (`docs/api-budget-design.md`
/// 4.1). Unknown until a reading arrives, and unknown again once its reset
/// has passed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApiBudget {
    reading: Option<crate::github::BudgetReading>,
    /// The reading's reset as a local instant (`anchor_reset`).
    reset_at: Option<DateTime<Local>>,
    /// Drawn once per window, so a pause and a limit in the same window resume
    /// at the same moment, and every check of it agrees.
    resume_jitter_secs: i64,
}

/// What `ApiBudget::observe` did with a reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// Older than the one held, or from a discarded answer it may not apply.
    Ignored,
    /// Replaced the held reading on the same token, or was the first.
    Updated,
    /// An applied answer on another token replaced the held reading.
    Switched,
}

/// `x-ratelimit-reset` as a local instant. The delay to the reset is taken in
/// the server's own time (`reset - Date`) and added to the local arrival, so a
/// skewed local clock neither pauses too long nor resumes into a refused
/// window; without a `Date` the local clock stands in. Clamped to one window.
pub fn anchor_reset(
    reading: &crate::github::BudgetReading,
    arrival: DateTime<Local>,
) -> DateTime<Local> {
    let server_now = reading.date.map_or(arrival.timestamp(), |d| d.timestamp());
    let delay = (reading.reset - server_now).clamp(0, RESET_WINDOW_SECS);
    arrival + chrono::Duration::seconds(delay)
}

impl ApiBudget {
    /// Take a reading under the order of `docs/api-budget-design.md` 4.1:
    /// only an applied answer may switch the token; on the same token a later
    /// reset is a newer window, and within one window the lower `remaining` is
    /// the newer reading. `applied` is false for an answer `accept_answer`
    /// discarded.
    pub fn observe(
        &mut self,
        reading: &crate::github::BudgetReading,
        applied: bool,
        arrival: DateTime<Local>,
        jitter: &mut dyn Jitter,
    ) -> Observed {
        let Some(held) = &self.reading else {
            if !applied {
                return Observed::Ignored;
            }
            self.replace(reading, arrival, jitter);
            return Observed::Updated;
        };
        if held.token_id != reading.token_id {
            if !applied {
                return Observed::Ignored;
            }
            self.replace(reading, arrival, jitter);
            return Observed::Switched;
        }
        match reading.reset.cmp(&held.reset) {
            std::cmp::Ordering::Greater => {
                self.replace(reading, arrival, jitter);
                Observed::Updated
            }
            std::cmp::Ordering::Equal if reading.remaining < held.remaining => {
                // The same window: its reset and jitter stand.
                self.reading = Some(reading.clone());
                Observed::Updated
            }
            _ => Observed::Ignored,
        }
    }

    fn replace(
        &mut self,
        reading: &crate::github::BudgetReading,
        arrival: DateTime<Local>,
        jitter: &mut dyn Jitter,
    ) {
        self.reset_at = Some(anchor_reset(reading, arrival));
        self.resume_jitter_secs = jitter.resume_secs().clamp(0, RESUME_JITTER_MAX_SECS);
        self.reading = Some(reading.clone());
    }

    /// The held reading, whether or not its window is over. Its token decides
    /// which later readings may apply.
    #[cfg(test)]
    pub fn reading(&self) -> Option<&crate::github::BudgetReading> {
        self.reading.as_ref()
    }

    /// The reading while its window lasts at `at`; `None` is unknown.
    pub fn known(&self, at: DateTime<Local>) -> Option<&crate::github::BudgetReading> {
        let reset_at = self.reset_at?;
        (reset_at > at).then_some(self.reading.as_ref()?)
    }

    /// The reset as a local instant.
    #[cfg(test)]
    pub fn reset_at(&self) -> Option<DateTime<Local>> {
        self.reset_at
    }

    /// When polling resumes after this window: the reset plus its jitter.
    pub fn resume_at(&self) -> Option<DateTime<Local>> {
        self.reset_at
            .map(|t| t + chrono::Duration::seconds(self.resume_jitter_secs))
    }

    /// Whether the next request would be refused at `at`: fewer points left
    /// than one poll costs. Treated as a known primary limit until the reset.
    pub fn exhausted(&self, cost: u32, at: DateTime<Local>) -> bool {
        self.known(at).is_some_and(|r| r.remaining < cost)
    }

    /// How far apart automatic polls of `cost` points go, judged at `at` (the
    /// last attempt): what is left above the reserve spread over the time to
    /// the reset, never past the resume. At or below the reserve, or with too
    /// little left for one poll, paused until the resume.
    pub fn pace(&self, cost: u32, at: DateTime<Local>) -> Pace {
        let (Some(reading), Some(resume)) = (self.known(at), self.resume_at()) else {
            return Pace::Unknown;
        };
        let reserve = u64::from(reading.limit) * BUDGET_RESERVE_PERCENT / 100;
        let remaining = u64::from(reading.remaining);
        if remaining <= reserve || reading.remaining < cost {
            return Pace::Paused { until: resume };
        }
        let to_reset = self.reset_at.map_or(0, |t| (t - at).num_seconds().max(0)) as u64;
        let spare = remaining - reserve;
        let secs = (u64::from(cost) * to_reset).div_ceil(spare) as i64;
        Pace::Every(secs.min((resume - at).num_seconds().max(0)))
    }
}

/// When the next automatic attempt may go at the earliest, beside the rule 6
/// interval. See `GithubBackoff`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hold {
    /// A fixed instant: a known reset plus its jitter, or a `retry-after`.
    At(DateTime<Local>),
    /// A step of the ladder, counted from the failure at `from`. Resolved
    /// against the configured interval when asked, which `App` does not hold.
    Ladder {
        from: DateTime<Local>,
        step: u32,
        factor: f64,
        at_least_secs: i64,
    },
}

/// The not-before and consecutive-failure count of `docs/api-budget-design.md`
/// 4.3. Kept beside `GithubFreshness`, not inside it, so `retry_due` keeps its
/// meaning: the not-before is a second condition, checked next to it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GithubBackoff {
    failures: u32,
    hold: Option<Hold>,
    /// Whether `hold` is a rate limit's, which a token change drops and which
    /// holds back `r` and `R`.
    hold_is_limit: bool,
}

/// Seconds the ladder waits after the `step`th consecutive failure: exactly
/// the interval at step 1, then doubling up to the cap, times the jitter
/// `factor` (applied after the cap), never under the interval.
pub fn backoff_secs(step: u32, interval_secs: i64, factor: f64) -> i64 {
    if step <= 1 {
        return interval_secs;
    }
    let doublings = (step - 1).min(32);
    let base = interval_secs
        .saturating_mul(1i64 << doublings)
        .min(BACKOFF_CAP_SECS);
    let factor = factor.clamp(BACKOFF_JITTER_MIN, BACKOFF_JITTER_MAX);
    ((base as f64 * factor).round() as i64).max(interval_secs)
}

impl GithubBackoff {
    /// The consecutive-failure count.
    #[cfg(test)]
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// A success clears both the count and the not-before (transition 3).
    pub fn succeeded(&mut self) {
        *self = Self::default();
    }

    /// An applied failure at `now`. `budget` already holds the reading that
    /// came with it. `Offline` and `Auth` say nothing about GitHub and leave
    /// everything as it was (transition 4); every other kind counts and
    /// replaces the not-before (transition 2).
    pub fn failed(
        &mut self,
        failure: &crate::github::Failure,
        now: DateTime<Local>,
        budget: &ApiBudget,
        jitter: &mut dyn Jitter,
    ) {
        use crate::github::{FailureKind, RateLimit};
        if matches!(failure.kind, FailureKind::Offline | FailureKind::Auth) {
            return;
        }
        self.failures = self.failures.saturating_add(1);
        let step = self.failures;
        let mut ladder = |at_least_secs| Hold::Ladder {
            from: now,
            step,
            // Step 1 is exactly the interval: no draw.
            factor: if step >= 2 {
                jitter.backoff_factor()
            } else {
                1.0
            },
            at_least_secs,
        };
        let (hold, is_limit) = match failure.limit {
            None => (ladder(0), false),
            Some(RateLimit::Primary) => match budget.known(now).and(budget.resume_at()) {
                Some(resume) => (Hold::At(resume), true),
                None => (ladder(0), true),
            },
            Some(RateLimit::Secondary {
                retry_after: Some(wait),
            }) => {
                let wait = chrono::Duration::from_std(wait)
                    .unwrap_or_else(|_| chrono::Duration::seconds(RESET_WINDOW_SECS));
                (Hold::At(now + wait), true)
            }
            Some(RateLimit::Secondary { retry_after: None }) => {
                (ladder(SECONDARY_LIMIT_MIN_SECS), true)
            }
        };
        self.hold = Some(hold);
        self.hold_is_limit = is_limit;
    }

    /// An applied answer came on another token: a rate limit belonged to the
    /// old credential and goes with it. The count describes GitHub and stays.
    pub fn account_switched(&mut self) {
        if self.hold_is_limit {
            self.hold = None;
            self.hold_is_limit = false;
        }
    }

    /// The not-before, resolved against the configured interval.
    pub fn not_before(&self, interval_secs: i64) -> Option<DateTime<Local>> {
        Some(match self.hold? {
            Hold::At(t) => t,
            Hold::Ladder {
                from,
                step,
                factor,
                at_least_secs,
            } => {
                let secs = backoff_secs(step, interval_secs, factor).max(at_least_secs);
                from + chrono::Duration::seconds(secs)
            }
        })
    }

    /// The not-before when it is a rate limit's.
    pub fn limit_until(&self, interval_secs: i64) -> Option<DateTime<Local>> {
        self.not_before(interval_secs)
            .filter(|_| self.hold_is_limit)
    }
}

/// Floor on the GitHub poll interval, whatever the config says, and the
/// interval while a repo is `Pending`. One poll at `repo_limit` 50 costs 3
/// points, so one instance at the floor spends 720 of the 5000-point hour:
/// comfortable for one instance, not for several on one account, which reach
/// the limit at seven (`docs/api-budget-design.md` section 3). The floor
/// stops a typo; the budget pacing (`ApiBudget::pace`) is what slows
/// polling when the account runs low.
pub(crate) const GITHUB_POLL_MIN: u64 = 15;

/// What the poll decisions need from the front end, built by
/// `App::poll_context`. `dirty` and `loading` stay apart: claiming an attempt
/// checks `dirty` only, so `r` during a load still polls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollContext {
    /// Watch mode is on.
    pub watch: bool,
    /// `-d`: no GitHub to poll.
    pub dirty: bool,
    /// A full load is running.
    pub loading: bool,
    /// A row the last poll returned shows CI `Pending`.
    pub any_pending: bool,
}

/// The GitHub sync: what the fleet poll knows and decides, kept together
/// because its parts must agree (the lock and the attempt ids, the hold and
/// the ladder, the budget and the pace).
pub struct GithubSync {
    /// How current the GitHub data is: data time, last attempt and outcome.
    ///
    /// A failure has to survive a keypress. It used to live only in a status
    /// message, which the next key clears, so a poll that had been failing
    /// for hours looked exactly like one that had just succeeded: the calendar
    /// and the CI lamps kept their last good values and nothing on screen said
    /// they had stopped moving. Rate limits are the common case and they last
    /// an hour. The header reads this.
    freshness: GithubFreshness,
    /// The account's GitHub budget as the newest reading stated it.
    budget: ApiBudget,
    /// The retry ladder and not-before beside `freshness`'s clock.
    backoff: GithubBackoff,
    /// Where the backoff and resume jitter come from; planted in tests.
    jitter: Box<dyn Jitter>,
    /// The configured GitHub poll interval in seconds, floored, and what one
    /// poll costs in points (`github::poll_cost` of `repo_limit`). Set once
    /// from `Config` at startup (`from_config`); neither changes while
    /// fleetz runs.
    interval_secs: i64,
    poll_cost: u32,
    /// When the poll in flight started, `None` when none is, so ticks do not
    /// stack them and a request that never returns can be given up on.
    /// Without the time, one hung call held the lock forever and CI status
    /// silently stopped updating for the rest of the session, with nothing on
    /// screen to say so.
    inflight_since: Option<Instant>,
    /// Set when a sweep sees a repo's unpushed count fall to zero, which means
    /// you just pushed. That is the one moment CI is about to change and
    /// nothing is `Pending` yet to poll harder on, so it forces the next poll
    /// rather than waiting out the interval.
    push_detected: bool,
}

impl GithubSync {
    /// A sync that has seen nothing yet, polling every `interval_secs` at
    /// `poll_cost` points a poll.
    pub fn new(interval_secs: i64, poll_cost: u32, jitter: Box<dyn Jitter>) -> Self {
        Self {
            freshness: GithubFreshness::default(),
            budget: ApiBudget::default(),
            backoff: GithubBackoff::default(),
            jitter,
            interval_secs,
            poll_cost,
            inflight_since: None,
            push_detected: false,
        }
    }

    /// The sync `config` asks for: its poll interval floored, priced at its
    /// `repo_limit` (D3). The only `Config` read of the poll decisions.
    pub fn from_config(config: &Config, jitter: Box<dyn Jitter>) -> Self {
        Self::new(
            Self::configured_poll_secs(config),
            crate::github::poll_cost(config.repo_limit),
            jitter,
        )
    }

    /// The configured poll interval, floored.
    pub(crate) fn configured_poll_secs(config: &Config) -> i64 {
        config.github_poll_interval.max(GITHUB_POLL_MIN) as i64
    }

    /// Whether the tick should start a background GitHub poll now.
    ///
    /// With watch on, once per `effective_interval` after the last attempt,
    /// whatever it ended in, or sooner for a detected push while polls succeed.
    /// With watch off, only the one attempt a cache-served launch is owed
    /// (decision D2): the data on screen came off the disk and nothing has
    /// confirmed it yet.
    pub fn poll_due(&self, now: DateTime<Local>, ctx: PollContext) -> bool {
        if ctx.dirty || ctx.loading || self.inflight() {
            return false;
        }
        let freshness = &self.freshness;
        if !ctx.watch {
            return freshness.awaits_first_attempt();
        }
        // Beside the interval, not inside it: a rate limit's reset, a
        // `retry-after`, a backoff step or a pause at the reserve.
        let held = self.not_before().is_some_and(|t| now < t);
        // A detected push jumps the queue: it is the one moment CI is about to
        // change with nothing `Pending` yet to notice. Not while polls fail, where
        // the next attempt waits out at least a full interval (rule 6), nor while
        // the budget slows polling; the next due poll serves it then.
        let slowed = self.slowed();
        let push = self.push_detected && !freshness.is_failing() && !slowed;
        !held && (push || freshness.retry_due(now, self.effective_interval(ctx)))
    }

    /// The earliest the next automatic attempt may go besides the interval: the
    /// later of the retry ladder's not-before and a pause at the reserve.
    fn not_before(&self) -> Option<DateTime<Local>> {
        let ladder = self.backoff.not_before(self.interval_secs);
        let pause = match self.pace() {
            Pace::Paused { until } => Some(until),
            _ => None,
        };
        ladder.max(pause)
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
    /// `not_before`, which `poll_due` checks beside this.
    pub fn effective_interval(&self, ctx: PollContext) -> i64 {
        let configured = self.interval_secs;
        let pace = match self.pace() {
            Pace::Every(secs) => secs,
            Pace::Unknown | Pace::Paused { .. } => 0,
        };
        let any_pending = ctx.any_pending;
        let floored = any_pending && !self.freshness.is_failing() && !self.slowed();
        let interval = if floored {
            GITHUB_POLL_MIN as i64
        } else {
            configured
        };
        interval.max(pace)
    }

    /// Take the in-flight lock at `now` and an attempt id for a new poll, or
    /// `None` when one is already running or `-d` (`dirty`) has no GitHub to
    /// poll.
    pub fn claim_attempt(&mut self, now: Instant, dirty: bool) -> Option<u64> {
        if self.inflight() || dirty {
            return None;
        }
        self.inflight_since = Some(now);
        Some(self.freshness.begin_attempt())
    }

    /// Give up on a background poll that has not answered within `limit`.
    /// True when it gave up.
    ///
    /// Recorded as a timed-out failure, which also advances the retry clock: the
    /// in-flight flag used to be dropped with nothing else changed, so the next
    /// tick started a second request while the first was still open (decision
    /// D4). The request itself is not cancelled; that is FZ-11.
    pub fn expire_stuck(&mut self, limit: Duration, now: DateTime<Local>) -> bool {
        if let Some(started) = self.inflight_since
            && started.elapsed() > limit
        {
            self.inflight_since = None;
            self.freshness.gave_up(limit, now);
            // A give-up is a `TimedOut` failure and climbs the ladder like one
            // (transition 5).
            if let Outcome::Failed(failure) = self.freshness.outcome.clone() {
                self.advance_ladder(&failure, now);
            }
            return true;
        }
        false
    }

    /// An answer to `attempt` arrived; true when it is the newest attempt's.
    ///
    /// The in-flight lock belongs to the newest attempt. A late answer from
    /// one given up on must not release it, or a second request stacks; the
    /// newest one's answer always does, even when it is discarded.
    pub fn answer_arrived(&mut self, attempt: u64) -> bool {
        let is_latest = self.freshness.is_latest(attempt);
        if is_latest {
            self.inflight_since = None;
        }
        is_latest
    }

    /// Record that `attempt`'s answer is being applied; false when a newer
    /// one was (`GithubFreshness::accept_answer`).
    pub fn accept_answer(&mut self, attempt: u64) -> bool {
        self.freshness.accept_answer(attempt)
    }

    /// A budget reading came with an answer that `applied` or was discarded.
    /// One on another token, from an applied answer, drops the old token's
    /// rate limit (transition 7). Call before the answer's outcome, which
    /// sets the not-before that follows it.
    pub fn observe_budget(
        &mut self,
        reading: Option<&crate::github::BudgetReading>,
        applied: bool,
        arrival: DateTime<Local>,
    ) {
        let Some(reading) = reading else {
            return;
        };
        if self
            .budget
            .observe(reading, applied, arrival, self.jitter.as_mut())
            == Observed::Switched
        {
            self.backoff.account_switched();
        }
    }

    /// An applied attempt came back with data at `now`.
    pub fn attempt_succeeded(&mut self, now: DateTime<Local>) {
        self.freshness.succeeded(now);
        self.backoff.succeeded();
    }

    /// An applied attempt failed at `now`: the ladder climbs, then the
    /// outcome is recorded.
    pub fn attempt_failed(&mut self, failure: crate::github::Failure, now: DateTime<Local>) {
        self.advance_ladder(&failure, now);
        self.freshness.failed(failure, now);
    }

    /// An applied attempt failed at `now`: advance the retry ladder.
    fn advance_ladder(&mut self, failure: &crate::github::Failure, now: DateTime<Local>) {
        self.backoff
            .failed(failure, now, &self.budget, self.jitter.as_mut());
    }

    /// What a landed load tells the budget and the retry ladder. A load that went
    /// to GitHub is an applied attempt (it supersedes every poll out), so its
    /// reading applies and its outcome counts; a cache read and a load with no
    /// fetch behind it tell them nothing.
    pub fn load_attempted(
        &mut self,
        source: &DatasetSource,
        reading: Option<&crate::github::BudgetReading>,
        now: DateTime<Local>,
    ) {
        match source {
            DatasetSource::Local | DatasetSource::Cache { .. } => {}
            DatasetSource::Network => {
                self.observe_budget(reading, true, now);
                self.backoff.succeeded();
            }
            DatasetSource::Fallback { failure, .. } => {
                self.observe_budget(reading, true, now);
                self.advance_ladder(failure, now);
            }
        }
    }

    /// A load landed (`GithubFreshness::loaded`).
    pub fn loaded(&mut self, source: &DatasetSource, now: DateTime<Local>) {
        self.freshness.loaded(source, now);
    }

    /// A sweep saw a push: force the next poll (`push_detected`).
    pub fn note_push(&mut self) {
        self.push_detected = true;
    }

    /// Clear the push shortcut as a poll starts; true when one was noted.
    pub fn take_push(&mut self) -> bool {
        std::mem::take(&mut self.push_detected)
    }

    pub fn freshness(&self) -> &GithubFreshness {
        &self.freshness
    }

    /// The budget's pace for polls of `poll_cost` points, judged at the last
    /// attempt, the moment polls are spaced from. Before any attempt there is
    /// nothing to space.
    pub fn pace(&self) -> Pace {
        match self.freshness.last_attempt {
            Some(at) => self.budget.pace(self.poll_cost, at),
            None => Pace::Unknown,
        }
    }

    /// Whether the budget stretches polling past the configured interval or
    /// pauses it, which suspends the `Pending` floor and the push shortcut
    /// (A2).
    pub fn slowed(&self) -> bool {
        match self.pace() {
            Pace::Unknown => false,
            Pace::Every(secs) => secs > self.interval_secs,
            Pace::Paused { .. } => true,
        }
    }

    /// Until when GitHub is known to refuse a request at `now`: a rate limit's
    /// not-before that has not passed, or too few points left for one poll
    /// (`docs/api-budget-design.md` 4.3, "Manual refresh"). `r` and `R` send
    /// nothing while this is `Some`.
    pub fn limit_until(&self, now: DateTime<Local>) -> Option<DateTime<Local>> {
        let limit = self
            .backoff
            .limit_until(self.interval_secs)
            .filter(|t| *t > now);
        let exhausted = self
            .budget
            .exhausted(self.poll_cost, now)
            .then(|| self.budget.resume_at())
            .flatten();
        limit.max(exhausted)
    }

    pub fn interval_secs(&self) -> i64 {
        self.interval_secs
    }

    #[cfg(test)]
    pub fn poll_cost(&self) -> u32 {
        self.poll_cost
    }

    /// Whether a poll is in flight.
    pub fn inflight(&self) -> bool {
        self.inflight_since.is_some()
    }

    /// The jitter stream, which the CI poll shares until it has its own
    /// owner (A1c).
    pub(crate) fn jitter_mut(&mut self) -> &mut dyn Jitter {
        self.jitter.as_mut()
    }
}

/// Where tests poke the state they used to write on `App` directly (D7).
#[cfg(test)]
impl GithubSync {
    pub fn set_jitter(&mut self, jitter: Box<dyn Jitter>) {
        self.jitter = jitter;
    }

    pub fn set_inflight_since(&mut self, since: Option<Instant>) {
        self.inflight_since = since;
    }

    pub fn inflight_since(&self) -> Option<Instant> {
        self.inflight_since
    }

    pub fn set_push_detected(&mut self, push: bool) {
        self.push_detected = push;
    }

    pub fn push_detected(&self) -> bool {
        self.push_detected
    }

    pub fn freshness_mut(&mut self) -> &mut GithubFreshness {
        &mut self.freshness
    }

    pub fn set_interval(&mut self, interval_secs: i64) {
        self.interval_secs = interval_secs;
    }

    pub fn budget(&self) -> &ApiBudget {
        &self.budget
    }

    pub fn backoff(&self) -> &GithubBackoff {
        &self.backoff
    }
}

#[cfg(test)]
mod tests;
