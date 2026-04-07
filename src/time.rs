use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone, Utc};

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

/// Resolve "auto" mode: return the cutoff for the previous time slot boundary.
pub fn auto_cutoff(work_start: u8, work_end: u8) -> (DateTime<Utc>, String) {
    let now = Local::now();
    let today = now.date_naive();
    let hour = now.hour();
    let slot = TimeSlot::from_hour(hour, work_start, work_end);

    let (cutoff_local, label) = match slot {
        TimeSlot::Evening => {
            // Currently evening — show since work_start today
            let t = today
                .and_time(NaiveTime::from_hms_opt(work_start as u32, 0, 0).unwrap());
            let label = format!("since {}  {}", today, slot.label(work_start, work_end));
            (t, label)
        }
        TimeSlot::Work => {
            // Currently working hours — show since yesterday evening (work_end)
            let yesterday = today - chrono::Duration::days(1);
            let t = yesterday
                .and_time(NaiveTime::from_hms_opt(work_end as u32, 0, 0).unwrap());
            let label = format!(
                "since {}  {}",
                yesterday,
                TimeSlot::Evening.label(work_start, work_end)
            );
            (t, label)
        }
        TimeSlot::Night => {
            // Currently night — show since yesterday work_end
            let yesterday = today - chrono::Duration::days(1);
            let t = yesterday
                .and_time(NaiveTime::from_hms_opt(work_end as u32, 0, 0).unwrap());
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
        s => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|e| format!("invalid date '{s}': {e}")),
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
        // Just verify it produces something reasonable — exact value depends on current time
        assert!(cutoff < Utc::now());
        assert!(label.contains("since"));
    }
}
