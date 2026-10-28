use chrono::{DateTime, Duration, Local, Months, NaiveDate, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

/// The three time slots in a day, derived from work_start/work_end config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeSlot {
    Work,
    Evening,
    Night,
}

impl TimeSlot {
    /// Determine which slot `hour` falls into.
    pub fn from_hour(hour: u32, work_start: u8, work_end: u8) -> Self {
        if hour >= work_end as u32 {
            Self::Evening
        } else if hour >= work_start as u32 {
            Self::Work
        } else {
            Self::Night
        }
    }

    pub fn label(&self, work_start: u8, work_end: u8) -> String {
        match self {
            Self::Work => format!("Work ({:02}-{:02})", work_start, work_end),
            Self::Evening => format!("Evening ({:02}-00)", work_end),
            Self::Night => format!("Night (00-{:02})", work_start),
        }
    }
}

/// Minimum for the user-adjustable window count. A count of 0 would mean
/// an empty window, so the floor is 1.
pub const WINDOW_N_MIN: u8 = 1;

/// Unit for the interactive time window. `Slot` reproduces the classic
/// auto mode (previous work/evening/night boundary); the rest are rolling
/// "now minus n units" windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowUnit {
    Slot,
    Hours,
    Days,
    Weeks,
    Months,
    Years,
}

impl WindowUnit {
    /// The largest count that reads naturally for this unit. Practical
    /// caps: they still allow useful overlap ("36 hours", "18 months")
    /// rather than snapping to the next unit. Slot ignores the count, so
    /// its max is a nominal 1.
    pub fn max_count(&self) -> u8 {
        match self {
            WindowUnit::Slot => 1,
            WindowUnit::Hours => 48,
            WindowUnit::Days => 31,
            WindowUnit::Weeks => 52,
            WindowUnit::Months => 24,
            WindowUnit::Years => 10,
        }
    }
}

/// The activity window selected in the TUI: show repos pushed within the
/// last `n` `unit`s. `n` is ignored when `unit` is `Slot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    pub n: u8,
    pub unit: WindowUnit,
}

impl Default for TimeWindow {
    fn default() -> Self {
        Self {
            n: 1,
            unit: WindowUnit::Slot,
        }
    }
}

impl TimeWindow {
    /// Construct with `n` clamped into the valid range, so persisted or
    /// hand-edited values can never produce an empty or absurd window.
    pub fn clamped(n: u8, unit: WindowUnit) -> Self {
        Self {
            n: n.clamp(WINDOW_N_MIN, unit.max_count()),
            unit,
        }
    }

    /// The cutoff instant for this window, evaluated against the real clock.
    pub fn cutoff(&self, work_start: u8, work_end: u8) -> DateTime<Utc> {
        self.cutoff_from(Utc::now(), work_start, work_end)
    }

    /// Deterministic core of `cutoff` (testable with a fixed `now`).
    /// Months/years use calendar-aware arithmetic and saturate rather than
    /// panic on impossible dates. `Slot` ignores `now`: it delegates to
    /// `auto_cutoff`, which reads the local clock itself.
    pub fn cutoff_from(&self, now: DateTime<Utc>, work_start: u8, work_end: u8) -> DateTime<Utc> {
        let n = self.n.clamp(WINDOW_N_MIN, self.unit.max_count()) as i64;
        match self.unit {
            WindowUnit::Slot => auto_cutoff(work_start, work_end).0,
            WindowUnit::Hours => now - Duration::hours(n),
            WindowUnit::Days => now - Duration::days(n),
            WindowUnit::Weeks => now - Duration::weeks(n),
            WindowUnit::Months => now.checked_sub_months(Months::new(n as u32)).unwrap_or(now),
            WindowUnit::Years => now
                .checked_sub_months(Months::new(n as u32 * 12))
                .unwrap_or(now),
        }
    }

    /// Header label: the slot label for `Slot`, otherwise "since 2 weeks".
    pub fn label(&self, work_start: u8, work_end: u8) -> String {
        let unit = match self.unit {
            WindowUnit::Slot => return auto_cutoff(work_start, work_end).1,
            WindowUnit::Hours => "hour",
            WindowUnit::Days => "day",
            WindowUnit::Weeks => "week",
            WindowUnit::Months => "month",
            WindowUnit::Years => "year",
        };
        let n = self.n.clamp(WINDOW_N_MIN, self.unit.max_count());
        let s = if n == 1 { "" } else { "s" };
        format!("since {n} {unit}{s}")
    }
}

/// Resolve "auto" mode: return the cutoff for the previous time slot boundary.
pub fn auto_cutoff(work_start: u8, work_end: u8) -> (DateTime<Utc>, String) {
    slot_cutoff_at(Local::now(), work_start, work_end)
}

/// The deterministic core of `auto_cutoff`, evaluated at `now`. The one
/// place a slot boundary is derived: the time window and the slot digest
/// (`digest::DigestWindow`) both read it, so they cannot disagree on where
/// the slot began.
pub fn slot_cutoff_at(
    now: DateTime<Local>,
    work_start: u8,
    work_end: u8,
) -> (DateTime<Utc>, String) {
    let today = now.date_naive();
    let hour = now.hour();
    let slot = TimeSlot::from_hour(hour, work_start, work_end);

    let (cutoff_local, label) = match slot {
        TimeSlot::Evening => {
            // Currently evening: show since work_start today
            let t = today.and_time(NaiveTime::from_hms_opt(work_start as u32, 0, 0).unwrap());
            let label = format!("since {}  {}", today, slot.label(work_start, work_end));
            (t, label)
        }
        TimeSlot::Work => {
            // Currently working hours: show since yesterday evening (work_end)
            let yesterday = today - chrono::Duration::days(1);
            let t = yesterday.and_time(NaiveTime::from_hms_opt(work_end as u32, 0, 0).unwrap());
            let label = format!(
                "since {}  {}",
                yesterday,
                TimeSlot::Evening.label(work_start, work_end)
            );
            (t, label)
        }
        TimeSlot::Night => {
            // Currently night: show since yesterday work_end
            let yesterday = today - chrono::Duration::days(1);
            let t = yesterday.and_time(NaiveTime::from_hms_opt(work_end as u32, 0, 0).unwrap());
            let label = format!(
                "since {}  {}",
                yesterday,
                TimeSlot::Evening.label(work_start, work_end)
            );
            (t, label)
        }
    };

    let cutoff_utc = Local
        .from_local_datetime(&cutoff_local)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    (cutoff_utc, label)
}

/// Parse a relative date string like "yesterday", "3d", "1w", or an absolute "2026-03-15".
pub fn parse_relative_date(input: &str) -> Result<NaiveDate, String> {
    let today = Local::now().date_naive();

    match input {
        "yesterday" => Ok(today - chrono::Duration::days(1)),
        s if s.ends_with('d') => {
            let n: i64 = s[..s.len() - 1]
                .parse()
                .map_err(|_| format!("invalid day count: {s}"))?;
            Ok(today - chrono::Duration::days(n))
        }
        s if s.ends_with('w') => {
            let n: i64 = s[..s.len() - 1]
                .parse()
                .map_err(|_| format!("invalid week count: {s}"))?;
            Ok(today - chrono::Duration::days(n * 7))
        }
        s => {
            NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("invalid date '{s}': {e}"))
        }
    }
}

/// Convert a NaiveDate to a UTC DateTime at midnight.
pub fn date_to_utc_midnight(date: NaiveDate) -> DateTime<Utc> {
    let local_midnight = date.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    Local
        .from_local_datetime(&local_midnight)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

/// Convert a NaiveDate to a UTC DateTime at end of day (23:59:59).
pub fn date_to_utc_end_of_day(date: NaiveDate) -> DateTime<Utc> {
    let local_eod = date.and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap());
    Local
        .from_local_datetime(&local_eod)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

use chrono::Timelike;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_slot_from_hour() {
        assert_eq!(TimeSlot::from_hour(3, 9, 18), TimeSlot::Night);
        assert_eq!(TimeSlot::from_hour(9, 9, 18), TimeSlot::Work);
        assert_eq!(TimeSlot::from_hour(14, 9, 18), TimeSlot::Work);
        assert_eq!(TimeSlot::from_hour(18, 9, 18), TimeSlot::Evening);
        assert_eq!(TimeSlot::from_hour(22, 9, 18), TimeSlot::Evening);
    }

    #[test]
    fn time_slot_labels() {
        assert_eq!(TimeSlot::Work.label(9, 18), "Work (09-18)");
        assert_eq!(TimeSlot::Evening.label(9, 18), "Evening (18-00)");
        assert_eq!(TimeSlot::Night.label(9, 18), "Night (00-09)");
    }

    #[test]
    fn parse_relative_yesterday() {
        let today = Local::now().date_naive();
        let yesterday = today - chrono::Duration::days(1);
        assert_eq!(parse_relative_date("yesterday").unwrap(), yesterday);
    }

    #[test]
    fn parse_relative_days() {
        let today = Local::now().date_naive();
        let expected = today - chrono::Duration::days(3);
        assert_eq!(parse_relative_date("3d").unwrap(), expected);
    }

    #[test]
    fn parse_relative_weeks() {
        let today = Local::now().date_naive();
        let expected = today - chrono::Duration::days(14);
        assert_eq!(parse_relative_date("2w").unwrap(), expected);
    }

    #[test]
    fn parse_absolute_date() {
        let d = parse_relative_date("2026-03-15").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 3, 15).unwrap());
    }

    #[test]
    fn parse_invalid_date_returns_error() {
        assert!(parse_relative_date("nope").is_err());
        assert!(parse_relative_date("xd").is_err());
    }

    #[test]
    fn auto_cutoff_returns_utc_datetime_and_label() {
        let (cutoff, label) = auto_cutoff(9, 18);
        // Just verify it produces something reasonable: exact value depends on current time
        assert!(cutoff < Utc::now());
        assert!(label.contains("since"));
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn window_cutoff_hours_days_weeks() {
        let now = utc("2026-07-13T12:00:00Z");
        let w = TimeWindow {
            n: 6,
            unit: WindowUnit::Hours,
        };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2026-07-13T06:00:00Z"));
        let w = TimeWindow {
            n: 3,
            unit: WindowUnit::Days,
        };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2026-07-10T12:00:00Z"));
        let w = TimeWindow {
            n: 2,
            unit: WindowUnit::Weeks,
        };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2026-06-29T12:00:00Z"));
    }

    #[test]
    fn window_cutoff_months_is_calendar_aware() {
        // March 31 minus one month clamps to February's last day.
        let now = utc("2026-03-31T10:00:00Z");
        let w = TimeWindow {
            n: 1,
            unit: WindowUnit::Months,
        };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2026-02-28T10:00:00Z"));
    }

    #[test]
    fn window_cutoff_years_via_months() {
        let now = utc("2026-07-13T12:00:00Z");
        let w = TimeWindow {
            n: 2,
            unit: WindowUnit::Years,
        };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2024-07-13T12:00:00Z"));
    }

    #[test]
    fn window_cutoff_slot_matches_auto_cutoff() {
        let w = TimeWindow {
            n: 1,
            unit: WindowUnit::Slot,
        };
        let (expected, _) = auto_cutoff(9, 18);
        let got = w.cutoff(9, 18);
        // Two separate now() reads: allow a small skew.
        assert!((got - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn window_clamped_bounds_n() {
        assert_eq!(TimeWindow::clamped(0, WindowUnit::Days).n, 1);
        assert_eq!(TimeWindow::clamped(99, WindowUnit::Days).n, 31);
        assert_eq!(TimeWindow::clamped(7, WindowUnit::Days).n, 7);
    }

    #[test]
    fn window_label_pluralizes() {
        assert_eq!(
            TimeWindow {
                n: 1,
                unit: WindowUnit::Days
            }
            .label(9, 18),
            "since 1 day"
        );
        assert_eq!(
            TimeWindow {
                n: 2,
                unit: WindowUnit::Weeks
            }
            .label(9, 18),
            "since 2 weeks"
        );
        assert_eq!(
            TimeWindow {
                n: 3,
                unit: WindowUnit::Months
            }
            .label(9, 18),
            "since 3 months"
        );
    }

    #[test]
    fn window_label_slot_uses_auto_label() {
        let w = TimeWindow {
            n: 1,
            unit: WindowUnit::Slot,
        };
        let (_, expected) = auto_cutoff(9, 18);
        assert_eq!(w.label(9, 18), expected);
    }

    #[test]
    fn window_default_is_one_slot() {
        let w = TimeWindow::default();
        assert_eq!(w.n, 1);
        assert_eq!(w.unit, WindowUnit::Slot);
    }

    #[test]
    fn window_unit_serde_roundtrip() {
        let w = TimeWindow {
            n: 2,
            unit: WindowUnit::Weeks,
        };
        let s = toml::to_string(&w).unwrap();
        assert!(s.contains("n = 2"));
        assert!(s.contains("unit = \"weeks\""));
        let back: TimeWindow = toml::from_str(&s).unwrap();
        assert_eq!(back, w);
    }

    #[test]
    fn window_unit_max_count_per_unit() {
        assert_eq!(WindowUnit::Slot.max_count(), 1);
        assert_eq!(WindowUnit::Hours.max_count(), 48);
        assert_eq!(WindowUnit::Days.max_count(), 31);
        assert_eq!(WindowUnit::Weeks.max_count(), 52);
        assert_eq!(WindowUnit::Months.max_count(), 24);
        assert_eq!(WindowUnit::Years.max_count(), 10);
    }

    #[test]
    fn window_clamped_bounds_per_unit() {
        assert_eq!(TimeWindow::clamped(40, WindowUnit::Days).n, 31);
        assert_eq!(TimeWindow::clamped(40, WindowUnit::Hours).n, 40);
        assert_eq!(TimeWindow::clamped(60, WindowUnit::Weeks).n, 52);
        assert_eq!(TimeWindow::clamped(0, WindowUnit::Hours).n, 1);
    }
}
