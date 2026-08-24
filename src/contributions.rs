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

/// Days the `recent` window re-asks for, today included.
///
/// GitHub answers a year-long `contributionsCollection` from a precomputed
/// index that lags behind the live one. Probed on 2026-08-24 at 12:19 CEST,
/// three commits pushed at 12:03 that day were already on the profile graph
/// and already visible to a one-day window (`totalContributions: 3`), while
/// the 2025-08-25..2026-08-24 window the grid is built from still reported
/// that day as 0. The green strip drew the zero it was handed, so a day's
/// work went missing until GitHub's own index caught up.
///
/// Seven rather than one because the lag has no documented bound, and a week
/// is still free: an alias with no `first:` connection adds no cost to the
/// query.
const RECENT_DAYS: i64 = 7;

/// The `[from, to]` window re-asked for at full freshness, laid over the year
/// windows. A subset of the newest one from `year_windows`, deliberately: the
/// point is to ask the same days a second way, not to reach days the year
/// windows miss.
pub fn recent_window(today: NaiveDate) -> (NaiveDate, NaiveDate) {
    (
        (today - Duration::days(RECENT_DAYS - 1)).max(grid_start(today)),
        today,
    )
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

/// A filled year of contribution counts.
#[derive(Debug, Clone)]
pub struct Calendar {
    start: NaiveDate,
    today: NaiveDate,
    counts: [u32; SLOTS],
    source: Source,
}

impl Calendar {
    pub fn new(today: NaiveDate, source: Source) -> Self {
        Self {
            start: grid_start(today),
            today,
            counts: [0; SLOTS],
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
    pub fn set(&mut self, date: NaiveDate, count: u32) {
        if let Some(slot) = self.slot(date) {
            self.counts[slot] = count;
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

    /// Shade step 0..=4 for a count. Quartiles of the calendar's own busiest
    /// day, matching GitHub's relative scaling: a quiet fleet still shows
    /// shape instead of one flat tone.
    pub fn intensity(&self, count: u32) -> u8 {
        if count == 0 {
            return 0;
        }
        let max = self.max().max(1);
        ((count as u64 * 4).div_ceil(max as u64)).clamp(1, 4) as u8
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

    #[test]
    fn intensity_buckets_by_quartile_of_the_busiest_day() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add(d("2026-01-05"), 100);
        assert_eq!(cal.intensity(0), 0, "an empty day is never shaded");
        assert_eq!(cal.intensity(1), 1, "one contribution is visible");
        assert_eq!(cal.intensity(25), 1, "top of the first quartile");
        assert_eq!(cal.intensity(26), 2);
        assert_eq!(cal.intensity(75), 3);
        assert_eq!(cal.intensity(100), 4, "the busiest day is the brightest");
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
}
