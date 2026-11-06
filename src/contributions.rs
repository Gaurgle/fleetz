//! The contribution grid: 53 columns of 7 days, the shape GitHub draws.
//!
//! Pure by design. Both producers (the GraphQL calendar and the local
//! `git log` histogram) fill the same structure through the same slot
//! arithmetic, so the two can never disagree about which cell a date is.

use chrono::{Datelike, Duration, NaiveDate};

/// Years of history the grid covers.
///
/// GitHub's own graph shows one. Fleetz shows more because a year of squares
/// leaves most of a wide pane empty, and because the account is older than
/// the window: the year before last holds real work that simply had nowhere
/// to be drawn. One GraphQL call still covers all of it -- see
/// `year_windows`.
pub const YEARS: i64 = 3;

/// Columns in the grid: 52 whole weeks per year, plus the partial one today
/// sits in. 364 is a whole number of weeks, so every year adds exactly 52
/// columns and the grid stays rectangular however far back it reaches.
pub const WEEKS: usize = 52 * YEARS as usize + 1;

/// How the strip's title names the span it is showing. Derived from `YEARS`
/// rather than written out, so a grid that reaches back three years cannot
/// keep announcing "the last year".
pub fn span_label() -> String {
    if YEARS == 1 {
        "the last year".to_string()
    } else {
        format!("the last {YEARS} years")
    }
}

/// Cells in the grid: 53 columns of 7 days. Slots past today exist so the
/// array is a fixed size, and are excluded from every aggregate.
pub const SLOTS: usize = WEEKS * 7;

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Where a calendar's numbers came from. They will not agree: GitHub counts
/// PRs, issues, reviews and private repos outside the fleet, the local source
/// counts commits in cloned repos. The pane names the active one for exactly
/// that reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Github,
    Local,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Local => "local",
        }
    }
}

/// The Sunday the grid opens on: the Sunday on or before `today - 364*YEARS`.
///
/// At `YEARS == 1` this reproduces GitHub's own window exactly. Probed on
/// 2026-08-18, the API opened its calendar on 2025-08-17, which is what
/// `year_windows` still puts its newest window on. Because 364 is a whole
/// number of weeks, the result is always `52 * YEARS` weeks plus a partial
/// one back from today, so the grid is always `WEEKS` columns wide.
pub fn grid_start(today: NaiveDate) -> NaiveDate {
    let back = today - Duration::days(364 * YEARS);
    back - Duration::days(back.weekday().num_days_from_sunday() as i64)
}

/// The `[from, to]` windows to ask GitHub for, newest first, tiling exactly
/// from `grid_start` to today.
///
/// `contributionsCollection` refuses a range longer than a year, so a
/// multi-year grid needs one window per year. They ride the same request as
/// aliased fields, which is why this is affordable: the full poll measured at
/// cost 13 / nodeCount 2625 both with four of these and with none, so the
/// price does not move. Fields, not requests, which is what the "never widen
/// the GitHub fetch" invariant permits.
///
/// Disjoint by construction, and the last one is clamped to `grid_start`
/// rather than running past it: whole windows would leave a few unqueried
/// days at the far left that could only ever render as zero.
pub fn year_windows(today: NaiveDate) -> Vec<(NaiveDate, NaiveDate)> {
    let start = grid_start(today);
    let mut windows = Vec::new();
    let mut to = today;
    while to >= start {
        let from = (to - Duration::days(364)).max(start);
        windows.push((from, to));
        to = from - Duration::days(1);
    }
    windows
}

/// Days merged from the `recent` payload, today included.
///
/// Fixed at seven regardless of how wide the window actually asked for, so
/// the merge never has to know which `from` the rotation picked. That matters
/// because a payload read back from the disk cache was built under a
/// different rotation than the one in force when it is parsed.
const RECENT_DAYS: i64 = 7;

/// How long one `from` date is held before the rotation moves on, and how
/// many distinct dates it cycles through.
///
/// GitHub caches a `contributionsCollection` response against the `from`
/// **date** alone. Probed 2026-08-24: `to` is not part of the key, nor is the
/// time component of `from`, and the entry survived two hours, eight pushes
/// and thirteen minutes of being left completely alone. So a query that asks
/// the same `from` every poll reads its own stale answer all day, which is
/// exactly what the strip was doing: 3 contributions on screen against 11 on
/// github.com.
///
/// Any `from` not asked for recently answers fresh, so the window rotates.
/// Holding each date five minutes bounds how stale today's cell can be, and
/// 358 dates put the full cycle at just over 29 hours, comfortably past any
/// entry lifetime observed. Both numbers matter: shorten the hold and the
/// cycle stops clearing the entry lifetime; shorten the cycle and the
/// rotation starts serving its own leftovers.
const ROTATION_HOLD_SECS: i64 = 300;
const ROTATION_SPAN: i64 = 358;
/// The hold bounds how stale today's cell can be, and it is the one cell
/// anybody actually looks at; the cycle has to clear any observed entry
/// lifetime. Raising either past these breaks the pair.
const _: () = assert!(ROTATION_HOLD_SECS <= 600 && ROTATION_SPAN * ROTATION_HOLD_SECS >= 24 * 3600);

/// Which slot of the rotation `now` falls in. Separate from `recent_window`
/// so the caller supplies the clock and the window stays a pure function.
pub fn rotation_tick(now: chrono::DateTime<chrono::Utc>) -> i64 {
    now.timestamp().div_euclid(ROTATION_HOLD_SECS)
}

/// The `[from, to]` window to re-ask for, laid over the year windows.
///
/// `to` is always today; only `from` rotates, because only `from` is in
/// GitHub's cache key. It reaches back at least `RECENT_DAYS` so the merge
/// range is always covered with margin, and never more than a year, which
/// `contributionsCollection` refuses.
pub fn recent_window(today: NaiveDate, tick: i64) -> (NaiveDate, NaiveDate) {
    let back = RECENT_DAYS + tick.rem_euclid(ROTATION_SPAN);
    ((today - Duration::days(back)).max(grid_start(today)), today)
}

/// The oldest day taken from a `recent` payload, whatever window produced it.
///
/// The window is always wider than this, so the padded week at its far edge
/// can never reach the merged range and zero a day only the year windows know
/// about.
pub fn recent_merge_floor(today: NaiveDate) -> NaiveDate {
    today - Duration::days(RECENT_DAYS - 1)
}

/// One repo's commits per grid slot. `u16` because no repo gets 65k commits
/// from one author in one day, and 742 bytes per repo is small enough to
/// keep on every `RepoStatus`.
#[derive(Debug, Clone, Copy)]
pub struct DailyCounts(pub [u16; SLOTS]);

impl Default for DailyCounts {
    fn default() -> Self {
        Self([0; SLOTS])
    }
}

/// Where a day's count falls among the active days of its calendar (FZ-91).
/// Steps by distribution, not by fraction of the busiest day, so one outlier
/// cannot flatten the rest of the year into the palest shade.
#[derive(Debug, Clone, PartialEq)]
pub enum ShadeScale {
    /// The 25th, 50th (median) and 75th percentiles: a count up to the
    /// first is step 1, up to the median 2, up to the third 3, above it 4.
    Quartiles([u32; 3]),
    /// Fewer than four distinct counts (a tiny sample, or ties) cannot fill
    /// four steps by percentile. They spread over the steps in order, the
    /// smallest at 1 and the largest at 4; one distinct count is step 4.
    Spread(Vec<u32>),
}

impl ShadeScale {
    pub fn step(&self, count: u32) -> u8 {
        if count == 0 {
            return 0;
        }
        match self {
            Self::Quartiles(bounds) => 1 + bounds.iter().filter(|&&b| count > b).count() as u8,
            Self::Spread(distinct) => {
                let k = distinct.len();
                let rank = distinct.partition_point(|&d| d <= count).max(1);
                (4 - (k.saturating_sub(rank) * 3) / k.saturating_sub(1).max(1)) as u8
            }
        }
    }
}

/// A filled year of contribution counts.
#[derive(Debug, Clone)]
pub struct Calendar {
    start: NaiveDate,
    today: NaiveDate,
    counts: [u32; SLOTS],
    /// GitHub's own shade for the day, where the payload carried one. Not
    /// drawn since FZ-91: the strip shades both sources by `shade_scale`.
    levels: [Option<u8>; SLOTS],
    source: Source,
}

impl Calendar {
    pub fn new(today: NaiveDate, source: Source) -> Self {
        Self {
            start: grid_start(today),
            today,
            counts: [0; SLOTS],
            levels: [None; SLOTS],
            source,
        }
    }

    pub fn source(&self) -> Source {
        self.source
    }

    pub fn start(&self) -> NaiveDate {
        self.start
    }

    /// The grid slot a date falls in, or None when it is outside the year.
    pub fn slot(&self, date: NaiveDate) -> Option<usize> {
        let offset = (date - self.start).num_days();
        (offset >= 0 && (offset as usize) < SLOTS).then_some(offset as usize)
    }

    /// The last slot carrying real data. Everything after it is future.
    pub fn today_slot(&self) -> usize {
        self.slot(self.today).unwrap_or(SLOTS - 1)
    }

    fn live(&self) -> usize {
        self.today_slot() + 1
    }

    /// Move `today` forward to the local date, so the strip gains the new
    /// day between polls (#78). Returns false, changing nothing, unless
    /// `today` is later than the calendar's own.
    ///
    /// When `grid_start` moves, the grid shifts back by those whole weeks so
    /// every date keeps its count and level; the oldest columns fall off.
    /// The days that become live are empty until a poll fills them. A jump
    /// wider than the grid keeps nothing, which is the same as a new empty
    /// calendar on that date. Allocation-free: it runs on the tick.
    pub fn advance_to(&mut self, today: NaiveDate) -> bool {
        if today <= self.today {
            return false;
        }
        let old_live = self.live();
        let start = grid_start(today);
        let shift = ((start - self.start).num_days() as usize).min(SLOTS);
        self.counts.copy_within(shift.., 0);
        self.counts[SLOTS - shift..].fill(0);
        self.levels.copy_within(shift.., 0);
        self.levels[SLOTS - shift..].fill(None);
        self.start = start;
        self.today = today;
        let new_days = old_live.saturating_sub(shift)..self.live();
        self.counts[new_days.clone()].fill(0);
        self.levels[new_days].fill(None);
        true
    }

    pub fn add(&mut self, date: NaiveDate, count: u32) {
        if let Some(slot) = self.slot(date) {
            self.counts[slot] += count;
        }
    }

    /// Record a day's count, replacing whatever was there.
    ///
    /// The GitHub source needs this where the local one needs `add`. Its
    /// calendar arrives as several one-year windows, and GitHub pads each
    /// window out to whole weeks, so two adjacent windows can both report the
    /// same day. Summing those would silently double it. The local source has
    /// the opposite requirement: its counts are genuinely summed across
    /// repos, so it keeps `add`.
    ///
    /// Also drops any level the day carried: the level was GitHub's verdict
    /// on the old count, and the `recent` window that overwrites a count
    /// cannot supply a usable one of its own (`derive_missing_levels`).
    pub fn set(&mut self, date: NaiveDate, count: u32) {
        if let Some(slot) = self.slot(date) {
            self.counts[slot] = count;
            self.levels[slot] = None;
        }
    }

    /// Record GitHub's own shade step for a day, 0..=4.
    pub fn set_level(&mut self, date: NaiveDate, level: u8) {
        if let Some(slot) = self.slot(date) {
            self.levels[slot] = Some(level.min(4));
        }
    }

    /// Shade step 0..=4 at a grid position: GitHub's where it gave one, else
    /// `intensity` of the count. Test-only since FZ-91: the strip draws every
    /// day by `shade_scale`, and this pins the level bookkeeping.
    #[cfg(test)]
    pub fn level_at(&self, week: usize, weekday: usize) -> u8 {
        let slot = week * 7 + weekday;
        match self.levels.get(slot) {
            Some(Some(level)) => *level,
            _ => self.intensity(self.counts.get(slot).copied().unwrap_or(0)),
        }
    }

    /// Give every unlevelled day from `from` to today a level on GitHub's
    /// own scale, read off the days around it that have one.
    ///
    /// GitHub scales a level to the window it was asked for, and trims
    /// outliers first by a rule this code cannot reproduce (measured against
    /// three years: a year whose busiest day had 83 contributions was scaled
    /// as if the top were 47, another with a 86-day as if it were 10). So a
    /// day the `recent` window overwrote cannot be bucketed by formula
    /// without landing on a different scale from its neighbours, and the
    /// `recent` window's own levels are scaled to a range that rotates every
    /// few minutes. The one scale that is knowable is the year window's, by
    /// example: the smallest count GitHub put at each level. A count lands
    /// on the highest level whose smallest example it reaches. A level with
    /// no example that year is skipped, and with no examples at all the day
    /// is left for `intensity`.
    pub fn derive_missing_levels(&mut self, from: NaiveDate) {
        let Some(first) = self.slot(from) else {
            return;
        };
        let window = first..self.live();
        let mut floor = [None::<u32>; 5];
        for slot in window.clone() {
            let count = self.counts[slot];
            if let Some(level) = self.levels[slot].filter(|_| count > 0) {
                let entry = &mut floor[level as usize];
                *entry = Some(entry.map_or(count, |f| f.min(count)));
            }
        }
        for slot in window {
            if self.levels[slot].is_some() || self.counts[slot] == 0 {
                continue;
            }
            let count = self.counts[slot];
            let level = (1..=4u8)
                .rev()
                .find(|l| floor[*l as usize].is_some_and(|f| count >= f));
            self.levels[slot] = level;
        }
    }

    /// Fold one repo's histogram in. Used once per visible repo to build the
    /// local calendar, so it must stay allocation-free.
    pub fn add_daily(&mut self, daily: &DailyCounts) {
        for (slot, n) in daily.0.iter().enumerate() {
            self.counts[slot] += *n as u32;
        }
    }

    pub fn total(&self) -> u32 {
        self.counts[..self.live()].iter().sum()
    }

    pub fn max(&self) -> u32 {
        self.counts[..self.live()]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// One column per week, Sunday first. `None` marks a slot past today, so
    /// the partial trailing week is expressed in exactly one place.
    pub fn weeks(&self) -> impl Iterator<Item = [Option<u32>; 7]> + '_ {
        let live = self.live();
        (0..WEEKS).map(move |w| {
            let mut column = [None; 7];
            for (weekday, cell) in column.iter_mut().enumerate() {
                let slot = w * 7 + weekday;
                if slot < live {
                    *cell = Some(self.counts[slot]);
                }
            }
            column
        })
    }

    /// The date and count at a grid position, or None past today.
    pub fn day_at(&self, week: usize, weekday: usize) -> Option<(NaiveDate, u32)> {
        let slot = week * 7 + weekday;
        (slot < self.live()).then(|| (self.start + Duration::days(slot as i64), self.counts[slot]))
    }

    /// The strip's one shade scale, for both sources, read off the active
    /// days (count above zero) of the whole calendar, so scrolling never
    /// reshades a day. Build it once per draw: it sorts every live day.
    pub fn shade_scale(&self) -> ShadeScale {
        let mut active: Vec<u32> = self.counts[..self.live()]
            .iter()
            .copied()
            .filter(|&n| n > 0)
            .collect();
        active.sort_unstable();
        let mut distinct = active.clone();
        distinct.dedup();
        if distinct.len() < 4 {
            return ShadeScale::Spread(distinct);
        }
        // Nearest-rank percentile: the smallest count with at least q
        // quarters of the active days at or below it.
        let percentile = |quarters: usize| active[(quarters * active.len()).div_ceil(4) - 1];
        ShadeScale::Quartiles([percentile(1), percentile(2), percentile(3)])
    }

    /// Shade step 0..=4 for a count on this calendar's `shade_scale`.
    pub fn intensity(&self, count: u32) -> u8 {
        self.shade_scale().step(count)
    }

    /// Which column each month opens in, for the label row.
    pub fn month_labels(&self) -> Vec<(usize, &'static str)> {
        let mut out = Vec::new();
        let mut previous = None;
        for week in 0..WEEKS {
            let date = self.start + Duration::days((week * 7) as i64);
            let month = date.month0() as usize;
            if previous != Some(month) {
                out.push((week, MONTH_NAMES[month]));
                previous = Some(month);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// At one year this reproduced GitHub's own window: probing
    /// `contributionCalendar` on 2026-08-18 opened its grid on 2025-08-17.
    /// The grid is `YEARS` long now and no longer shares a start with it, so
    /// what has to hold is that the newest *window* still reaches back a full
    /// year from today. If that drifts, the recent year stops matching what
    /// github.com draws for the same account.
    #[test]
    fn the_newest_window_still_covers_githubs_own_year() {
        let today = d("2026-08-18");
        let (from, to) = year_windows(today)[0];
        assert_eq!(to, today, "the newest window ends today");
        assert!(from <= d("2025-08-19"), "a full year back, got {from}");
    }

    #[test]
    fn grid_start_is_always_a_sunday_and_always_weeks_columns() {
        // Walk a full week of "today"s so every weekday is covered.
        for offset in 0..7 {
            let today = d("2026-08-16") + chrono::Duration::days(offset);
            let start = grid_start(today);
            assert_eq!(start.weekday(), chrono::Weekday::Sun, "today {today}");
            let span = (today - start).num_days();
            assert_eq!(span / 7, 52 * YEARS, "today {today} spans {span} days");
            assert!((span as usize) < SLOTS);
        }
    }

    /// The windows must tile `[grid_start, today]` exactly: a gap leaves
    /// columns that can only ever render zero, and an overlap would double a
    /// day if `parse_calendar` ever went back to summing them.
    #[test]
    fn year_windows_tile_the_grid_with_no_gap_and_no_overlap() {
        for offset in 0..7 {
            let today = d("2026-08-16") + chrono::Duration::days(offset);
            let windows = year_windows(today);
            assert_eq!(windows[0].1, today, "newest window ends today");
            assert_eq!(
                windows.last().unwrap().0,
                grid_start(today),
                "oldest window must reach the grid's first day"
            );
            for pair in windows.windows(2) {
                let (newer, older) = (pair[0], pair[1]);
                assert_eq!(
                    older.1 + Duration::days(1),
                    newer.0,
                    "windows must abut exactly"
                );
            }
            for (from, to) in &windows {
                assert!(
                    (*to - *from).num_days() < 365,
                    "GitHub rejects a range longer than a year"
                );
            }
        }
    }

    #[test]
    fn slots_outside_the_year_are_rejected() {
        let today = d("2026-08-18");
        let start = grid_start(today);
        let cal = Calendar::new(today, Source::Local);
        assert_eq!(cal.slot(start), Some(0));
        assert_eq!(
            cal.slot(start - Duration::days(1)),
            None,
            "day before the grid opens"
        );
        assert_eq!(cal.slot(today), Some((today - start).num_days() as usize));
    }

    #[test]
    fn add_ignores_dates_outside_the_year() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add(d("2026-08-18"), 5);
        cal.add(d("2020-01-01"), 99);
        assert_eq!(cal.total(), 5);
    }

    /// The grid opens on a Sunday by construction, so the FIRST column is
    /// always complete. Only the last one is short.
    #[test]
    fn first_week_is_complete_and_last_week_stops_at_today() {
        // 2026-08-18 is a Tuesday: Sun, Mon, Tue present, Wed..Sat absent.
        let cal = Calendar::new(d("2026-08-18"), Source::Github);
        let weeks: Vec<_> = cal.weeks().collect();
        assert_eq!(weeks.len(), WEEKS);
        assert!(weeks[0].iter().all(|c| c.is_some()), "no leading gap");
        let last = weeks[WEEKS - 1];
        assert!(last[0].is_some() && last[1].is_some() && last[2].is_some());
        assert!(last[3].is_none() && last[6].is_none());
    }

    #[test]
    fn day_at_maps_back_to_a_real_date() {
        let today = d("2026-08-18");
        let mut cal = Calendar::new(today, Source::Local);
        // Tuesday of the first column, whichever date that now is.
        let tuesday = grid_start(today) + Duration::days(2);
        cal.add(tuesday, 3);
        assert_eq!(cal.day_at(0, 2), Some((tuesday, 3)));
        assert_eq!(cal.day_at(WEEKS - 1, 6), None, "past today");
    }

    /// FZ-91, replacing `intensity_buckets_by_quartile_of_the_busiest_day`
    /// on Andreas's word: the scale is the distribution of the active days,
    /// not quarters of the busiest one.
    #[test]
    fn intensity_steps_by_the_quartiles_of_the_active_days() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        // Nine active days, 1 to 9 commits: quartiles 3, 5 (the median), 7.
        for n in 1..=9u32 {
            cal.add(d("2026-01-05") + Duration::days(n as i64), n);
        }
        assert_eq!(cal.shade_scale(), ShadeScale::Quartiles([3, 5, 7]));
        assert_eq!(cal.intensity(0), 0, "an empty day is never shaded");
        assert_eq!(cal.intensity(1), 1, "one contribution is visible");
        assert_eq!(cal.intensity(3), 1, "top of the first quartile");
        assert_eq!(cal.intensity(4), 2);
        assert_eq!(cal.intensity(5), 2, "the median day is step 2");
        assert_eq!(cal.intensity(6), 3);
        assert_eq!(cal.intensity(7), 3, "top of the third quartile");
        assert_eq!(cal.intensity(8), 4);
        assert_eq!(cal.intensity(9), 4, "the busiest day is the brightest");
    }

    /// The old scale took quarters of the busiest day, so one 500-commit
    /// day pressed every ordinary day into step 1.
    #[test]
    fn one_outlier_day_leaves_four_steps_across_the_ordinary_days() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        for n in 1..=8u32 {
            cal.add(d("2026-02-01") + Duration::days(n as i64), n);
        }
        cal.add(d("2026-03-01"), 500);
        let steps: std::collections::BTreeSet<u8> = (1..=8).map(|n| cal.intensity(n)).collect();
        assert_eq!(steps.into_iter().collect::<Vec<_>>(), vec![1, 2, 3, 4]);
        assert_eq!(cal.intensity(500), 4);
    }

    #[test]
    fn the_median_active_day_is_step_two_and_zero_days_step_zero() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Github);
        for (i, n) in [2u32, 40, 9, 1, 15].into_iter().enumerate() {
            cal.set(d("2026-04-01") + Duration::days(i as i64), n);
        }
        cal.set(d("2026-04-10"), 0);
        assert_eq!(cal.intensity(9), 2, "9 is the median of 1, 2, 9, 15, 40");
        let empty = cal.slot(d("2026-04-10")).unwrap();
        assert_eq!(
            cal.level_at(empty / 7, empty % 7),
            0,
            "a zero day is step 0"
        );
        assert_eq!(cal.intensity(0), 0);
    }

    /// Fewer than four active days cannot be split by percentile: the
    /// distinct counts spread over the steps, lowest at 1, highest at 4.
    #[test]
    fn fewer_than_four_active_days_spread_over_the_steps_in_order() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add(d("2026-05-01"), 3);
        assert_eq!(cal.intensity(3), 4, "a lone day is the brightest");
        cal.add(d("2026-05-02"), 10);
        assert_eq!((cal.intensity(3), cal.intensity(10)), (1, 4));
        cal.add(d("2026-05-03"), 6);
        assert_eq!(
            [3, 6, 10].map(|n| cal.intensity(n)),
            [1, 3, 4],
            "three days keep their order"
        );
        // Ties count once: four days on three distinct counts still spread.
        cal.add(d("2026-05-04"), 6);
        assert_eq!([3, 6, 10].map(|n| cal.intensity(n)), [1, 3, 4]);
    }

    #[test]
    fn an_empty_year_has_an_empty_scale() {
        let cal = Calendar::new(d("2026-08-18"), Source::Local);
        assert_eq!(cal.shade_scale(), ShadeScale::Spread(Vec::new()));
        assert_eq!(cal.intensity(0), 0);
    }

    fn at(cal: &Calendar, date: NaiveDate) -> u8 {
        let slot = cal.slot(date).unwrap();
        cal.level_at(slot / 7, slot % 7)
    }

    /// A level from the payload beats the formula; a fresh count for the
    /// same day discards it, since it was GitHub's verdict on the old count.
    #[test]
    fn a_github_level_wins_over_the_formula_until_the_count_changes() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Github);
        cal.set(d("2026-08-01"), 100);
        cal.set(d("2026-08-10"), 12);
        assert_eq!(at(&cal, d("2026-08-10")), 1, "formula: 12 of 100");
        cal.set_level(d("2026-08-10"), 3);
        assert_eq!(at(&cal, d("2026-08-10")), 3, "github said 3");
        cal.set(d("2026-08-10"), 13);
        assert_eq!(
            at(&cal, d("2026-08-10")),
            1,
            "a new count drops the old verdict"
        );
        assert_eq!(
            cal.level_at(WEEKS + 5, 0),
            0,
            "past the grid is unshaded, not a panic"
        );
    }

    /// An unlevelled day takes the highest level whose smallest example that
    /// year it reaches, on GitHub's scale rather than the formula's. A level
    /// with no example is skipped over, not interpolated.
    #[test]
    fn derive_missing_levels_reads_the_years_scale_off_its_examples() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Github);
        for (date, count, level) in [
            ("2026-08-10", 3, 1),
            ("2026-08-11", 12, 2),
            ("2026-08-12", 14, 2),
            ("2026-08-13", 38, 4),
        ] {
            cal.set(d(date), count);
            cal.set_level(d(date), level);
        }
        // The recent window overwrote these, so they have counts and no level.
        cal.set(d("2026-08-17"), 30);
        cal.set(d("2026-08-18"), 1);
        cal.set(d("2026-08-16"), 0);
        cal.derive_missing_levels(d("2026-01-01"));
        assert_eq!(
            at(&cal, d("2026-08-17")),
            2,
            "30 reaches 12 but not 38, and 3 has no example"
        );
        assert_eq!(
            at(&cal, d("2026-08-18")),
            1,
            "1 is below every example (the level-1 floor is 3), so the formula shades it"
        );
        assert_eq!(at(&cal, d("2026-08-16")), 0, "an empty day stays empty");
        assert_eq!(at(&cal, d("2026-08-13")), 4, "levelled days are untouched");
    }

    /// With no examples at all (a payload from before levels were fetched)
    /// nothing is derived and every day is drawn by the formula.
    #[test]
    fn derive_missing_levels_without_examples_leaves_the_formula_in_charge() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Github);
        cal.set(d("2026-08-10"), 40);
        cal.set(d("2026-08-17"), 10);
        cal.derive_missing_levels(d("2026-01-01"));
        assert_eq!(at(&cal, d("2026-08-17")), 1, "10 of 40 by formula");
        assert_eq!(at(&cal, d("2026-08-10")), 4);
    }

    #[test]
    fn intensity_of_an_empty_year_never_divides_by_zero() {
        let cal = Calendar::new(d("2026-08-18"), Source::Github);
        assert_eq!(cal.max(), 0);
        assert_eq!(cal.intensity(0), 0);
    }

    #[test]
    fn a_single_contribution_year_shows_that_day_at_full_brightness() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add(d("2026-03-12"), 1);
        assert_eq!(cal.intensity(1), 4);
    }

    #[test]
    fn month_labels_land_on_the_column_the_month_opens_in() {
        let cal = Calendar::new(d("2026-08-18"), Source::Github);
        let labels = cal.month_labels();
        assert_eq!(labels[0], (0, "Aug"), "grid opens 2023-08-20");
        assert_eq!(labels[1].1, "Sep");
        // Twelve months per year, plus the partial one the grid opens in.
        assert_eq!(labels.len(), 12 * YEARS as usize + 1);
        assert_eq!(labels.last().unwrap().1, "Aug");
        // Strictly increasing columns, one label per column at most.
        assert!(labels.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn add_daily_folds_a_per_repo_histogram_into_the_grid() {
        let mut counts = DailyCounts::default();
        counts.0[0] = 2;
        counts.0[366] = 4;
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add_daily(&counts);
        cal.add_daily(&counts);
        assert_eq!(cal.total(), 12);
        assert_eq!(cal.day_at(0, 0).unwrap().1, 4);
    }

    /// Slots after today hold zero but must never reach the total, or a
    /// second repo's stale tail would inflate the count.
    #[test]
    fn slots_past_today_are_excluded_from_total_and_max() {
        let mut counts = DailyCounts::default();
        counts.0[SLOTS - 1] = 9;
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add_daily(&counts);
        assert_eq!(cal.total(), 0);
        assert_eq!(cal.max(), 0);
    }

    /// The property the rotation exists for: a poll must not re-ask a `from`
    /// GitHub already has a cached answer against. Consecutive holds have to
    /// differ, or the rotation is decoration.
    #[test]
    fn consecutive_holds_ask_different_from_dates() {
        let today = d("2026-08-24");
        let a = recent_window(today, 100).0;
        let b = recent_window(today, 101).0;
        assert_ne!(a, b, "two holds in a row asked the same question");
    }

    /// The cycle has to outlast a cached entry, or the rotation comes back
    /// round to its own leftovers. Probed 2026-08-24, one entry was still
    /// serving a two-hour-old answer after eight pushes and thirteen minutes
    /// untouched, so the bar is "hours, generously".
    #[test]
    fn the_full_cycle_outlasts_a_cached_entry() {
        let today = d("2026-08-24");
        let seen: std::collections::HashSet<NaiveDate> = (0..ROTATION_SPAN)
            .map(|t| recent_window(today, t).0)
            .collect();
        assert_eq!(
            seen.len() as i64,
            ROTATION_SPAN,
            "the rotation repeats a date inside one cycle"
        );
        let cycle_hours = ROTATION_SPAN * ROTATION_HOLD_SECS / 3600;
        assert!(
            cycle_hours >= 24,
            "full cycle is only {cycle_hours}h, short enough to serve its own leftovers"
        );
    }

    /// `contributionsCollection` refuses a range longer than a year, so no
    /// point in the rotation may produce one.
    #[test]
    fn no_rotation_point_asks_for_more_than_a_year() {
        let today = d("2026-08-24");
        for tick in 0..ROTATION_SPAN {
            let (from, to) = recent_window(today, tick);
            assert!(
                (to - from).num_days() <= 364,
                "tick {tick} spans {} days",
                (to - from).num_days()
            );
        }
    }

    /// The merge takes a fixed seven days whatever the rotation asked for, and
    /// the window must always cover them with room to spare. Without the
    /// margin, the padded week at the window's far edge could reach into the
    /// merged range and zero a day only the year windows know about.
    #[test]
    fn every_window_covers_the_merged_days_with_margin() {
        let today = d("2026-08-24");
        let floor = recent_merge_floor(today);
        for tick in 0..ROTATION_SPAN {
            let (from, to) = recent_window(today, tick);
            assert!(
                from < floor,
                "tick {tick}: window starts inside the merged range"
            );
            assert_eq!(to, today, "tick {tick}: window must reach today");
        }
    }

    /// The merge floor must not move with the rotation. A payload read back
    /// from the disk cache was built under whatever rotation was in force when
    /// it was written, which the parser has no way to recover; if the clamp
    /// depended on that, a cached payload would be merged against the wrong
    /// floor.
    #[test]
    fn the_merge_floor_does_not_depend_on_the_rotation() {
        let today = d("2026-08-24");
        assert_eq!(recent_merge_floor(today), today - Duration::days(6));
    }

    /// The tick has to actually advance with the clock, and hold steady inside
    /// one slot.
    #[test]
    fn the_tick_advances_once_per_hold() {
        use chrono::TimeZone;
        let at = |s: i64| chrono::Utc.timestamp_opt(s, 0).single().unwrap();
        // Aligned to a hold boundary, so "one second before the next slot"
        // means what it says.
        let base = 1_787_000_000 - 1_787_000_000 % ROTATION_HOLD_SECS;
        assert_eq!(
            rotation_tick(at(base)),
            rotation_tick(at(base + ROTATION_HOLD_SECS - 1)),
            "the tick moved inside a single hold"
        );
        assert_ne!(
            rotation_tick(at(base)),
            rotation_tick(at(base + ROTATION_HOLD_SECS)),
            "the tick failed to advance at the hold boundary"
        );
    }

    /// #78: the poll stamps `today`, so after midnight the strip kept
    /// drawing yesterday as its last live square until the next poll. A
    /// weekday advance adds the new day empty and keeps every count.
    #[test]
    fn advancing_a_weekday_adds_the_new_day_empty() {
        let today = d("2026-08-18"); // Tuesday
        let mut cal = Calendar::new(today, Source::Github);
        cal.set(today, 5);
        cal.set_level(today, 2);
        cal.set(today - Duration::days(1), 9);
        cal.set_level(today - Duration::days(1), 4);
        let old_slot = cal.today_slot();
        let tomorrow = today + Duration::days(1);
        // A stale shade on the future slot must not survive onto the new day.
        cal.set_level(tomorrow, 2);

        assert!(cal.advance_to(tomorrow));
        assert_eq!(cal.today_slot(), old_slot + 1);
        let slot = cal.slot(tomorrow).unwrap();
        assert_eq!(cal.day_at(slot / 7, slot % 7), Some((tomorrow, 0)));
        assert_eq!(at(&cal, tomorrow), 0);
        let slot = cal.slot(today).unwrap();
        assert_eq!(cal.day_at(slot / 7, slot % 7), Some((today, 5)));
        assert_eq!(at(&cal, today), 2);
        let yesterday = today - Duration::days(1);
        let slot = cal.slot(yesterday).unwrap();
        assert_eq!(cal.day_at(slot / 7, slot % 7), Some((yesterday, 9)));
        assert_eq!(at(&cal, yesterday), 4);
        assert_eq!(cal.source(), Source::Github);
    }

    #[test]
    fn advancing_to_the_same_or_an_earlier_day_changes_nothing() {
        let today = d("2026-08-18");
        let mut cal = Calendar::new(today, Source::Github);
        cal.set(today, 5);
        cal.set_level(today, 2);
        let (start, slot) = (cal.start(), cal.today_slot());
        assert!(!cal.advance_to(today));
        assert!(!cal.advance_to(today - Duration::days(1)));
        assert_eq!((cal.start(), cal.today_slot()), (start, slot));
        assert_eq!(cal.day_at(slot / 7, slot % 7), Some((today, 5)));
        assert_eq!(at(&cal, today), 2);
    }

    /// Crossing into a Sunday moves `grid_start` a week, so the grid shifts
    /// back one column and every date keeps its own count and level.
    #[test]
    fn advancing_into_a_sunday_shifts_the_grid_a_week() {
        let today = d("2026-08-22"); // Saturday
        let mut cal = Calendar::new(today, Source::Github);
        let old_start = cal.start();
        let marker = old_start + Duration::days(7);
        cal.set(marker, 77);
        cal.set_level(marker, 3);
        cal.set(today, 6);
        cal.set_level(today, 1);
        // The old slot the new Sunday lands on after the shift: a shift that
        // left it in place would draw last Sunday's shade on the new day.
        let last_sunday = today - Duration::days(6);
        let new_sunday_slot = (last_sunday - old_start).num_days() as usize;
        cal.set(last_sunday, 8);
        cal.set_level(last_sunday, 4);

        let sunday = today + Duration::days(1);
        assert!(cal.advance_to(sunday));
        assert_eq!(cal.start(), old_start + Duration::days(7));
        assert_eq!(cal.slot(marker), Some(0));
        assert_eq!(cal.day_at(0, 0), Some((marker, 77)));
        assert_eq!(cal.level_at(0, 0), 3);
        let slot = cal.slot(today).unwrap();
        assert_eq!(cal.day_at(slot / 7, slot % 7), Some((today, 6)));
        assert_eq!(at(&cal, today), 1);
        assert_eq!(cal.today_slot(), slot + 1);
        let slot = cal.slot(sunday).unwrap();
        assert_eq!(slot, new_sunday_slot);
        assert_eq!(cal.day_at(slot / 7, slot % 7), Some((sunday, 0)));
        assert_eq!(at(&cal, sunday), 0);
        assert_eq!(at(&cal, last_sunday), 4, "last Sunday moved with its level");
        assert_eq!(cal.total(), 77 + 6 + 8);
    }

    /// A jump wider than the grid keeps nothing: the calendar starts empty
    /// on the new date and the next poll refills it.
    #[test]
    fn advancing_past_the_whole_grid_starts_empty() {
        let today = d("2026-08-18");
        let mut cal = Calendar::new(today, Source::Github);
        cal.set(today, 5);
        let later = today + Duration::days(SLOTS as i64 + 30);
        assert!(cal.advance_to(later));
        assert_eq!(cal.start(), grid_start(later));
        assert_eq!(cal.slot(later), Some(cal.today_slot()));
        assert_eq!(cal.total(), 0);
        assert_eq!(cal.source(), Source::Github);
    }
}
