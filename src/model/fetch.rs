use chrono::DateTime;

/// Why a `git fetch` failed, grouped by what you would do about it. Read from
/// git's stderr under the C locale `git_cmd` sets (`classify_fetch_error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchFailureKind {
    /// No progress for `FETCH_STALL_LIMIT`.
    TimedOut,
    /// Credentials wanted or refused. Prompts are disabled, so a remote that
    /// asks for a password fails here instead of hanging.
    Auth,
    /// ssh refused the key (`Permission denied (publickey)`): the fix is the
    /// key or the agent, not a password. fleetz fetches with `BatchMode=yes`,
    /// so a passphrase key that is not in the agent lands here.
    Ssh,
    /// The network, DNS or the host itself.
    Unreachable,
    /// The remote is missing or is not a repository.
    NoRemote,
    /// Anything else, including git failing to start.
    Other,
    /// Received more than the automatic-fetch cap (`fetch_max_mb`, FZ-35b)
    /// and was cut. Not an error: the repo is held until `f` fetches it
    /// uncapped or a fetch succeeds. The detail is the bytes received
    /// (`FetchFailure::received_bytes`).
    TooBig,
}

/// A failed fetch: its kind, and at most git's first error line with any URL
/// userinfo removed (rule 8). Never the raw stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchFailure {
    pub kind: FetchFailureKind,
    pub detail: Option<String>,
}

/// The detail of a `TooBig` failure: `<bytes> bytes received`.
const TOO_BIG_SUFFIX: &str = " bytes received";

impl FetchFailure {
    /// A fetch cut at the cap after receiving `bytes` (FZ-35b).
    pub fn too_big(bytes: u64) -> Self {
        Self {
            kind: FetchFailureKind::TooBig,
            detail: Some(format!("{bytes}{TOO_BIG_SUFFIX}")),
        }
    }

    /// What a `TooBig` fetch had received when it was cut; `None` for any
    /// other kind.
    pub fn received_bytes(&self) -> Option<u64> {
        if self.kind != FetchFailureKind::TooBig {
            return None;
        }
        self.detail
            .as_deref()?
            .strip_suffix(TOO_BIG_SUFFIX)?
            .parse()
            .ok()
    }
}

/// How a fetch for one checkout ended. `NotAttempted` is also what a sweep
/// that did not fetch reports: requested is not successful (rule 1), and not
/// requested says nothing at all (rule 2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FetchOutcome {
    #[default]
    NotAttempted,
    Succeeded,
    Failed(FetchFailure),
}

/// Which part of a fetch a progress update is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchPhase {
    /// The server building the pack (`remote: Counting objects`,
    /// `remote: Compressing objects`, ...): no bytes yet.
    Remote,
    /// The pack arriving: `Receiving objects`, or `Unpacking objects` for a
    /// fetch small enough to be unpacked loose.
    Receiving,
    /// `Resolving deltas` after the pack has arrived.
    Resolving,
}

/// One progress update of a running fetch, read from git's stderr.
/// `objects_total` is 0 when git gave only a count (`Enumerating objects:
/// 1234`). `bytes` is what has been received so far: git prints it on the
/// `Receiving objects` lines only, so later updates carry the last value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchProgress {
    pub phase: FetchPhase,
    pub objects_done: u64,
    pub objects_total: u64,
    pub bytes: u64,
}

/// The three fetch facts of `docs/fetch-outcome-contract.md` for one
/// checkout: when its remote refs were last actually updated, when a fetch
/// last finished, and how that ended. The same shape as
/// `app::GithubFreshness`, for the same reason: a fetch that was asked for is
/// not one that worked, and only a success may move the time the counts are
/// measured against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchFacts {
    /// When a fetch last succeeded. At load, from a non-empty `FETCH_HEAD`
    /// (`git::last_successful_fetch_at`); `None` is unknown and is never
    /// drawn as a time.
    pub last_success: Option<DateTime<chrono::Local>>,
    /// When a fetch last finished, however it ended.
    pub last_attempt: Option<DateTime<chrono::Local>>,
    pub outcome: FetchOutcome,
    /// The latest progress of a fetch running now (FZ-35a), `None` when none
    /// is or it has not reported yet. On the facts rather than beside them
    /// because the row's badges read the facts and nothing else of the
    /// fetch, and because `carried_into` then keeps it across a reload that
    /// lands mid-fetch. In memory only; cleared when the fetch lands.
    pub progress: Option<FetchProgress>,
    /// GitHub's `diskUsage` (kB) for this checkout's remote when a fetch of
    /// it last succeeded in this session (FZ-35b), the baseline the
    /// pre-check measures growth from. `None` is unknown. In memory only.
    pub disk_baseline: Option<u64>,
    /// How much the remote grew past the cap since `disk_baseline`, in
    /// bytes, while the pre-check holds the repo (`assess_growth`). Every
    /// automatic fetch and `F` skip it until `f` or a success.
    pub grown: Option<u64>,
}

/// The growth in bytes from `baseline_kb` to `current_kb`, when it is more
/// than `cap_bytes`. GitHub's `diskUsage` is in kilobytes.
fn growth_over_cap(
    baseline_kb: Option<u64>,
    current_kb: Option<u64>,
    cap_bytes: Option<u64>,
) -> Option<u64> {
    let cap = cap_bytes?;
    let growth = current_kb?.checked_sub(baseline_kb?)?.saturating_mul(1024);
    (growth > cap).then_some(growth)
}

/// Why a repo's automatic fetches wait for `f` (FZ-35b), with the size the
/// row names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeHold {
    /// GitHub's `diskUsage` grew by this many bytes, more than the cap,
    /// since the last successful fetch: `fetch ~N MB, press f`.
    Grown(u64),
    /// A fetch was cut at the cap after receiving this many bytes
    /// (`FetchFailureKind::TooBig`): `fetch over N MB, press f`.
    OverCap(u64),
}

impl FetchFacts {
    /// What a freshly loaded row knows: the disk's last success, no attempt.
    pub fn from_disk(last_success: Option<std::time::SystemTime>) -> Self {
        Self {
            last_success: last_success.map(DateTime::from),
            ..Self::default()
        }
    }

    /// A fetch for this checkout finished. Only a success advances
    /// `last_success` (rule 3); `NotAttempted` changes nothing (rule 2), not
    /// even the progress of a fetch still running. A finished one ends it.
    pub fn record(&mut self, outcome: &FetchOutcome, now: DateTime<chrono::Local>) {
        match outcome {
            FetchOutcome::NotAttempted => return,
            FetchOutcome::Succeeded => {
                self.last_success = Some(now);
                self.grown = None;
            }
            FetchOutcome::Failed(_) => {}
        }
        self.last_attempt = Some(now);
        self.outcome = outcome.clone();
        self.progress = None;
    }

    /// The failure, while the last fetch failed. Behind and ahead are then
    /// counted against the remote refs of `last_success` (rule 5).
    pub fn failure(&self) -> Option<&FetchFailure> {
        match &self.outcome {
            FetchOutcome::Failed(failure) => Some(failure),
            _ => None,
        }
    }

    /// The pre-check (FZ-35b): hold the repo when GitHub's `diskUsage` now
    /// (`current_kb`) exceeds `disk_baseline` by more than `cap_bytes`.
    /// A missing current value, an unknown baseline, an unchanged or
    /// shrunken size, or no cap decide nothing, and lift an earlier hold of
    /// this kind: the in-fetch cap still bounds the fetch.
    pub fn assess_growth(&mut self, current_kb: Option<u64>, cap_bytes: Option<u64>) {
        self.grown = growth_over_cap(self.disk_baseline, current_kb, cap_bytes);
    }

    /// Whether automatic fetches and `F` skip this repo, and the size to
    /// name. A fetch cut at the cap is the hold the row saw happen, so it is
    /// named over a growth GitHub reports.
    pub fn size_hold(&self) -> Option<SizeHold> {
        if let Some(failure) = self.failure()
            && failure.kind == FetchFailureKind::TooBig
        {
            return Some(SizeHold::OverCap(failure.received_bytes().unwrap_or(0)));
        }
        self.grown.map(SizeHold::Grown)
    }

    /// Carry what this session learned about fetching across a load that
    /// rebuilt the row (`R`, a time-window apply). Without it a window apply,
    /// which fetches nothing, wiped a failure off the row and drew stale
    /// counts as current, and an `R` whose own fetch failed forgot the last
    /// success the session had seen.
    ///
    /// `self` is the replaced row's facts, `loaded` the new row's: the disk's
    /// last success plus any fetch the load ran. A disk success newer than
    /// anything this session saw (a `git fetch` run in a terminal) is a
    /// success all the same; the load's own fetch is recorded last, and only
    /// when it is newer than the session's last attempt.
    pub fn carried_into(&self, loaded: &FetchFacts) -> FetchFacts {
        let mut kept = self.clone();
        let newer = |at: DateTime<chrono::Local>, than: Option<DateTime<chrono::Local>>| {
            than.is_none_or(|t| at > t)
        };
        if let Some(at) = loaded.last_success
            && newer(at, kept.last_success)
            && newer(at, kept.last_attempt)
        {
            kept.record(&FetchOutcome::Succeeded, at);
            // The size baseline of that success: the load's own fetch knows
            // it, a `git fetch` run in a terminal does not.
            kept.disk_baseline = loaded.disk_baseline;
        }
        // Only when newer than what the session already has: an `R` whose
        // fetch finished before a sweep's fetch of the same repo landed must
        // not move `last_attempt` back or put its older outcome on top.
        if let Some(at) = loaded.last_attempt
            && newer(at, kept.last_attempt)
        {
            kept.record(&loaded.outcome, at);
            if loaded.outcome == FetchOutcome::Succeeded {
                kept.disk_baseline = loaded.disk_baseline;
            }
        }
        kept
    }
}

#[cfg(test)]
mod tests;
