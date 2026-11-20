use chrono::{DateTime, Datelike, Days, FixedOffset, Local, Months, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PeriodSpec {
    Today,
    ThisWeek,
    LastWeek,
    ThisMonth,
    LastMonth,
    Custom { start_ms: i64, end_ms: i64 },
}

/// Half-open range `[start_ms, end_ms)` in epoch milliseconds (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodRange {
    pub start_ms: i64,
    pub end_ms: i64,
}

pub fn resolve(spec: &PeriodSpec, now: DateTime<FixedOffset>) -> PeriodRange {
    let today = now.date_naive();
    let week = week_start(today);
    let month = month_start(today);
    match *spec {
        PeriodSpec::Today => days(now, today, 1),
        PeriodSpec::ThisWeek => days(now, week, 7),
        PeriodSpec::LastWeek => days(now, week - Days::new(7), 7),
        PeriodSpec::ThisMonth => months(now, month, 1),
        PeriodSpec::LastMonth => months(now, month - Months::new(1), 1),
        PeriodSpec::Custom { start_ms, end_ms } => PeriodRange { start_ms, end_ms },
    }
}

/// The calendar period immediately before `spec`; for `Custom`, the same length right before it.
pub fn previous(spec: &PeriodSpec, now: DateTime<FixedOffset>) -> PeriodRange {
    let today = now.date_naive();
    let week = week_start(today);
    let month = month_start(today);
    match *spec {
        PeriodSpec::Today => days(now, today - Days::new(1), 1),
        PeriodSpec::ThisWeek => days(now, week - Days::new(7), 7),
        PeriodSpec::LastWeek => days(now, week - Days::new(14), 7),
        PeriodSpec::ThisMonth => months(now, month - Months::new(1), 1),
        PeriodSpec::LastMonth => months(now, month - Months::new(2), 1),
        PeriodSpec::Custom { start_ms, end_ms } => PeriodRange { start_ms: start_ms - (end_ms - start_ms), end_ms: start_ms },
    }
}

/// UTC offset of the machine's local time zone, in seconds.
pub fn local_offset_secs() -> i32 {
    Local::now().offset().local_minus_utc()
}

/// Monday on or before `d`.
pub(crate) fn week_start(d: NaiveDate) -> NaiveDate {
    d - Days::new(d.weekday().num_days_from_monday() as u64)
}

pub(crate) fn month_start(d: NaiveDate) -> NaiveDate {
    d.with_day(1).expect("day 1 exists")
}

/// Epoch ms of local midnight at the start of `d`, in `now`'s offset.
pub(crate) fn local_midnight_ms(now: DateTime<FixedOffset>, d: NaiveDate) -> i64 {
    d.and_time(NaiveTime::MIN)
        .and_local_timezone(*now.offset())
        .single()
        .expect("fixed offset is unambiguous")
        .timestamp_millis()
}

fn days(now: DateTime<FixedOffset>, start: NaiveDate, n: u64) -> PeriodRange {
    PeriodRange { start_ms: local_midnight_ms(now, start), end_ms: local_midnight_ms(now, start + Days::new(n)) }
}

fn months(now: DateTime<FixedOffset>, start: NaiveDate, n: u32) -> PeriodRange {
    PeriodRange { start_ms: local_midnight_ms(now, start), end_ms: local_midnight_ms(now, start + Months::new(n)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-21T10:00:00+07:00").unwrap()
    }
    fn ms(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }

    #[test]
    fn this_week_starts_monday_local() {
        let r = resolve(&PeriodSpec::ThisWeek, now());
        assert_eq!(r.start_ms, ms("2026-11-16T00:00:00+07:00"));
        assert_eq!(r.end_ms, ms("2026-11-23T00:00:00+07:00"));
    }

    #[test]
    fn last_month_range() {
        let r = resolve(&PeriodSpec::LastMonth, now());
        assert_eq!(r, PeriodRange { start_ms: ms("2026-10-01T00:00:00+07:00"), end_ms: ms("2026-11-01T00:00:00+07:00") });
    }

    #[test]
    fn today_and_previous_day() {
        assert_eq!(
            resolve(&PeriodSpec::Today, now()),
            PeriodRange { start_ms: ms("2026-11-21T00:00:00+07:00"), end_ms: ms("2026-11-22T00:00:00+07:00") }
        );
        assert_eq!(
            previous(&PeriodSpec::Today, now()),
            PeriodRange { start_ms: ms("2026-11-20T00:00:00+07:00"), end_ms: ms("2026-11-21T00:00:00+07:00") }
        );
    }

    #[test]
    fn previous_calendar_periods() {
        assert_eq!(previous(&PeriodSpec::ThisWeek, now()), resolve(&PeriodSpec::LastWeek, now()));
        assert_eq!(
            previous(&PeriodSpec::LastWeek, now()),
            PeriodRange { start_ms: ms("2026-11-02T00:00:00+07:00"), end_ms: ms("2026-11-09T00:00:00+07:00") }
        );
        assert_eq!(previous(&PeriodSpec::ThisMonth, now()), resolve(&PeriodSpec::LastMonth, now()));
        assert_eq!(
            previous(&PeriodSpec::LastMonth, now()),
            PeriodRange { start_ms: ms("2026-09-01T00:00:00+07:00"), end_ms: ms("2026-10-01T00:00:00+07:00") }
        );
    }

    #[test]
    fn last_month_wraps_year() {
        let jan = DateTime::parse_from_rfc3339("2027-01-05T10:00:00+07:00").unwrap();
        assert_eq!(
            resolve(&PeriodSpec::LastMonth, jan),
            PeriodRange { start_ms: ms("2026-12-01T00:00:00+07:00"), end_ms: ms("2027-01-01T00:00:00+07:00") }
        );
    }

    #[test]
    fn custom_previous_is_same_length_immediately_before() {
        let spec = PeriodSpec::Custom { start_ms: 1_000, end_ms: 4_000 };
        assert_eq!(resolve(&spec, now()), PeriodRange { start_ms: 1_000, end_ms: 4_000 });
        assert_eq!(previous(&spec, now()), PeriodRange { start_ms: -2_000, end_ms: 1_000 });
    }

    #[test]
    fn period_spec_json_shape() {
        assert_eq!(serde_json::to_string(&PeriodSpec::ThisWeek).unwrap(), r#"{"kind":"thisWeek"}"#);
        let c: PeriodSpec = serde_json::from_str(r#"{"kind":"custom","startMs":1,"endMs":2}"#).unwrap();
        assert_eq!(c, PeriodSpec::Custom { start_ms: 1, end_ms: 2 });
        let r = serde_json::to_string(&PeriodRange { start_ms: 1, end_ms: 2 }).unwrap();
        assert_eq!(r, r#"{"startMs":1,"endMs":2}"#);
    }
}
