//! The contribution grid: 53 columns of 7 days, the shape GitHub draws.
//!
//! Pure by design. Both producers (the GraphQL calendar and the local
//! `git log` histogram) fill the same structure through the same slot
//! arithmetic, so the two can never disagree about which cell a date is.

use chrono::{Datelike, Duration, NaiveDate};

/// Columns in the grid. GitHub returns exactly 53 and so do we.
pub const WEEKS: usize = 53;

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

/// The Sunday the grid opens on: the Sunday on or before `today - 364d`.
///
/// Reproduces GitHub's own window exactly. Probed on 2026-08-18, the API
/// opened its calendar on 2025-08-17, which is what this returns. Because
/// 364 is a whole number of weeks, the result is always 52 weeks plus a
/// partial one back from today, so the grid is always 53 columns wide.
pub fn grid_start(today: NaiveDate) -> NaiveDate {
    let back = today - Duration::days(364);
    back - Duration::days(back.weekday().num_days_from_sunday() as i64)
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

    /// The exact start the live API returned when this was designed: probing
    /// `contributionCalendar` on 2026-08-18 opened the grid on 2025-08-17.
    /// If this drifts, the two sources stop sharing a grid.
    #[test]
    fn grid_start_matches_the_probed_github_calendar() {
        assert_eq!(grid_start(d("2026-08-18")), d("2025-08-17"));
    }

    #[test]
    fn grid_start_is_always_a_sunday_and_always_53_columns() {
        // Walk a full week of "today"s so every weekday is covered.
        for offset in 0..7 {
            let today = d("2026-08-16") + chrono::Duration::days(offset);
            let start = grid_start(today);
            assert_eq!(start.weekday(), chrono::Weekday::Sun, "today {today}");
            let span = (today - start).num_days();
            assert_eq!(span / 7, 52, "today {today} spans {span} days");
            assert!((span as usize) < SLOTS);
        }
    }

    #[test]
    fn slots_outside_the_year_are_rejected() {
        let cal = Calendar::new(d("2026-08-18"), Source::Local);
        assert_eq!(cal.slot(d("2025-08-17")), Some(0));
        assert_eq!(cal.slot(d("2025-08-16")), None, "day before the grid opens");
        assert_eq!(cal.slot(d("2026-08-18")), Some(366));
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
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add(d("2025-08-19"), 3); // Tuesday of the first column
        assert_eq!(cal.day_at(0, 2), Some((d("2025-08-19"), 3)));
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
        assert_eq!(labels[0], (0, "Aug"), "grid opens 2025-08-17");
        assert_eq!(labels[1].1, "Sep");
        // Thirteen months are touched: Aug 2025 through Aug 2026.
        assert_eq!(labels.len(), 13);
        assert_eq!(labels[12].1, "Aug");
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
