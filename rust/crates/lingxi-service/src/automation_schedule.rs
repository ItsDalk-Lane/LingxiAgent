//! 自动任务的纯时间计算；触发收据、执行和通知由各自的权威服务负责。
//!
//! cron 沿用主机本地日历，游标使用 Unix 毫秒。搜索始终从下一个 UTC
//! 整分钟开始，避免夏令时回拨时本地时间设置器将游标倒退到已过去的时间。

use chrono::{Datelike, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use serde_json::Value;

pub const MIN_EVERY_INTERVAL_MS: u64 = 60_000;
pub const CRON_SEARCH_MINUTES: u64 = 366 * 24 * 60;
pub const FAILURE_BACKOFF_MS: [u64; 5] = [0, 60_000, 300_000, 900_000, 3_600_000];

/// 注入本地日历转换时所需的最小事实，weekday 的 0/7 均表示周日。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalMinute {
    pub minute: u32,
    pub hour: u32,
    pub day: u32,
    pub month: u32,
    pub weekday: u32,
}

impl LocalMinute {
    fn validate(self) -> Result<Self, String> {
        if self.minute > 59
            || self.hour > 23
            || !(1..=31).contains(&self.day)
            || !(1..=12).contains(&self.month)
            || self.weekday > 7
        {
            return Err("automation local time conversion returned invalid calendar fields".into());
        }
        Ok(Self {
            weekday: self.weekday % 7,
            ..self
        })
    }
}

/// 一次执行结束后的游标事实；disable=true 时保留诊断游标但不再调度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunCursor {
    pub next_run_at_ms: Option<u64>,
    pub consecutive_errors: u32,
    pub disable: bool,
}

pub fn next_run(kind: &str, schedule: &Value, from_ms: u64) -> Result<Option<u64>, String> {
    next_run_with_local(kind, schedule, from_ms, host_local_minute)
}

/// 日历映射可注入，测试无需修改进程时区或安装第二份时区数据库。
/// 映射仅影响 cron；带偏移的一次性时间与间隔计划都是绝对时间。
pub fn next_run_with_local<F>(
    kind: &str,
    schedule: &Value,
    from_ms: u64,
    mut local_at: F,
) -> Result<Option<u64>, String>
where
    F: FnMut(u64) -> Result<LocalMinute, String>,
{
    match kind {
        "at" => {
            validate_timestamp(from_ms)?;
            let target = parse_at(schedule)?;
            Ok((target > from_ms).then_some(target))
        }
        "every" => {
            validate_timestamp(from_ms)?;
            let interval = every_interval(schedule)?;
            let target = from_ms
                .checked_add(interval)
                .ok_or("automation interval exceeds supported timestamp range")?;
            validate_timestamp(target)?;
            Ok(Some(target))
        }
        "cron" => {
            validate_timestamp(from_ms)?;
            let expression = schedule
                .as_str()
                .ok_or("cron schedule must be a five-field expression")?;
            let fields = CronFields::parse(expression)?;
            let start = (from_ms / MIN_EVERY_INTERVAL_MS)
                .checked_add(1)
                .and_then(|minute| minute.checked_mul(MIN_EVERY_INTERVAL_MS))
                .ok_or("automation cron exceeds supported timestamp range")?;
            for index in 0..CRON_SEARCH_MINUTES {
                let candidate = start
                    .checked_add(index * MIN_EVERY_INTERVAL_MS)
                    .ok_or("automation cron exceeds supported timestamp range")?;
                validate_timestamp(candidate)?;
                if fields.matches(local_at(candidate)?.validate()?) {
                    return Ok(Some(candidate));
                }
            }
            Ok(None)
        }
        _ => Err(format!("unsupported automation schedule type: {kind}")),
    }
}

pub fn next_after_result(
    kind: &str,
    schedule: &Value,
    completed_ms: u64,
    success: bool,
    previous_consecutive_errors: u32,
) -> Result<RunCursor, String> {
    next_after_result_with_local(
        kind,
        schedule,
        completed_ms,
        success,
        previous_consecutive_errors,
        host_local_minute,
    )
}

/// 间隔和 cron 均从真实完成时间重算，错过多个时点不会生成补跑队列。
pub fn next_after_result_with_local<F>(
    kind: &str,
    schedule: &Value,
    completed_ms: u64,
    success: bool,
    previous_consecutive_errors: u32,
    local_at: F,
) -> Result<RunCursor, String>
where
    F: FnMut(u64) -> Result<LocalMinute, String>,
{
    let normal_next = next_run_with_local(kind, schedule, completed_ms, local_at)?;
    let consecutive_errors = if success {
        0
    } else {
        previous_consecutive_errors.saturating_add(1)
    };
    let next_run_at_ms = if success {
        normal_next
    } else {
        let backoff =
            FAILURE_BACKOFF_MS[(consecutive_errors as usize).min(FAILURE_BACKOFF_MS.len() - 1)];
        let retry_at = completed_ms
            .checked_add(backoff)
            .ok_or("automation backoff exceeds supported timestamp range")?;
        validate_timestamp(retry_at)?;
        Some(normal_next.map_or(retry_at, |normal| normal.max(retry_at)))
    };
    Ok(RunCursor {
        next_run_at_ms,
        consecutive_errors,
        // 现役一次性任务失败后也禁用，不能由退避游标暗中变为重复任务。
        disable: kind == "at",
    })
}

pub fn every_interval(schedule: &Value) -> Result<u64, String> {
    let value = match schedule {
        Value::Number(number) => {
            let value = number
                .as_f64()
                .ok_or("every schedule must contain positive milliseconds")?;
            if !value.is_finite() || value <= 0.0 || value >= u64::MAX as f64 {
                return Err("every schedule must contain positive milliseconds".into());
            }
            // 沿用 Date 毫秒精度；小于一分钟的合法正数统一钳制到一分钟。
            value.max(MIN_EVERY_INTERVAL_MS as f64).trunc() as u64
        }
        Value::String(text) => {
            let value = decimal_prefix(text)?;
            if value == 0 {
                return Err("every schedule must contain positive milliseconds".into());
            }
            value.max(MIN_EVERY_INTERVAL_MS)
        }
        _ => return Err("every schedule must contain positive milliseconds".into()),
    };
    Ok(value)
}

fn validate_timestamp(value: u64) -> Result<i64, String> {
    let value = i64::try_from(value)
        .map_err(|_| "automation timestamp exceeds supported range".to_string())?;
    Utc.timestamp_millis_opt(value)
        .single()
        .ok_or_else(|| "automation timestamp exceeds supported range".to_string())?;
    Ok(value)
}

fn host_local_minute(value: u64) -> Result<LocalMinute, String> {
    let time = Local
        .timestamp_millis_opt(validate_timestamp(value)?)
        .single()
        .ok_or("automation local timestamp exceeds supported range")?;
    Ok(LocalMinute {
        minute: time.minute(),
        hour: time.hour(),
        day: time.day(),
        month: time.month(),
        weekday: time.weekday().num_days_from_sunday(),
    })
}

fn parse_at(schedule: &Value) -> Result<u64, String> {
    if let Some(value) = schedule.as_u64() {
        validate_timestamp(value)?;
        return Ok(value);
    }
    let value = schedule
        .as_str()
        .map(str::trim)
        .ok_or("at schedule must contain a valid date")?;
    let timestamp = if let Ok(time) = chrono::DateTime::parse_from_rfc3339(value) {
        time.timestamp_millis()
    } else if let Ok(time) = chrono::DateTime::parse_from_rfc2822(value) {
        time.timestamp_millis()
    } else if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        // 与现役 ISO 日期的解释保持一致：只有日期时使用 UTC 零点。
        date.and_hms_opt(0, 0, 0)
            .ok_or("at schedule must contain a valid date")?
            .and_utc()
            .timestamp_millis()
    } else {
        let naive = [
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%dT%H:%M",
            "%Y-%m-%d %H:%M:%S%.f",
            "%Y-%m-%d %H:%M",
        ]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .ok_or("at schedule must contain a valid date")?;
        match Local.from_local_datetime(&naive) {
            LocalResult::Single(time) => time.timestamp_millis(),
            LocalResult::Ambiguous(first, second) => {
                first.timestamp_millis().min(second.timestamp_millis())
            }
            LocalResult::None => {
                return Err(
                    "at schedule local time does not exist; include an explicit UTC offset".into(),
                )
            }
        }
    };
    let timestamp = u64::try_from(timestamp)
        .map_err(|_| "at schedule predates supported Unix timestamp range".to_string())?;
    validate_timestamp(timestamp)?;
    Ok(timestamp)
}

fn decimal_prefix(text: &str) -> Result<u64, String> {
    let text = text
        .trim_start()
        .strip_prefix('+')
        .unwrap_or(text.trim_start());
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return Err("automation schedule contains an invalid positive integer".into());
    }
    digits
        .parse()
        .map_err(|_| "automation schedule integer exceeds supported range".into())
}

struct CronFields {
    values: [Vec<u32>; 5],
    day_restricted: bool,
    weekday_restricted: bool,
}

impl CronFields {
    fn parse(expression: &str) -> Result<Self, String> {
        let parts: Vec<&str> = expression.split_whitespace().collect();
        if parts.len() < 5 {
            return Err("cron schedule requires five fields".into());
        }
        // 现役解释器读取前五项；保留既有导入数据的尾随字段兼容性。
        let ranges = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];
        let mut values: [Vec<u32>; 5] = Default::default();
        for (index, (min, max)) in ranges.into_iter().enumerate() {
            values[index] = cron_field(parts[index], min, max, index == 4)?;
        }
        Ok(Self {
            values,
            day_restricted: parts[2] != "*",
            weekday_restricted: parts[4] != "*",
        })
    }

    fn matches(&self, local: LocalMinute) -> bool {
        let [minutes, hours, days, months, weekdays] = &self.values;
        let day_matches = days.contains(&local.day);
        let weekday_matches = weekdays.contains(&local.weekday);
        let calendar_day_matches = if self.day_restricted && self.weekday_restricted {
            day_matches || weekday_matches
        } else {
            day_matches && weekday_matches
        };
        minutes.contains(&local.minute)
            && hours.contains(&local.hour)
            && months.contains(&local.month)
            && calendar_day_matches
    }
}

fn cron_field(field: &str, min: u32, max: u32, weekday: bool) -> Result<Vec<u32>, String> {
    let mut values = Vec::new();
    let effective_max = if weekday { 7 } else { max };
    for segment in field.split(',') {
        let (lo, hi, step) = if segment == "*" {
            (u64::from(min), u64::from(max), 1)
        } else if let Some(step) = segment.strip_prefix("*/") {
            (u64::from(min), u64::from(max), decimal_prefix(step)?)
        } else if let Some((lo, rest)) = segment.split_once('-') {
            let (hi, step) = rest.split_once('/').unwrap_or((rest, "1"));
            if lo.is_empty()
                || hi.is_empty()
                || step.is_empty()
                || !lo.chars().all(|character| character.is_ascii_digit())
                || !hi.chars().all(|character| character.is_ascii_digit())
                || !step.chars().all(|character| character.is_ascii_digit())
            {
                return Err(format!("invalid cron range: {segment}"));
            }
            (
                decimal_prefix(lo)?,
                decimal_prefix(hi)?,
                decimal_prefix(step)?,
            )
        } else {
            // 兼容现役 parseInt 的十进制前缀读取，不把尾随兼容内容变成新权限。
            let number = decimal_prefix(segment)?;
            (number, number, 1)
        };
        if step == 0 || lo > hi || lo < u64::from(min) || hi > u64::from(effective_max) {
            return Err(format!("invalid cron field: {segment}"));
        }
        let mut value = lo;
        while value <= hi {
            values.push(if weekday && value == 7 {
                0
            } else {
                value as u32
            });
            let Some(next) = value.checked_add(step) else {
                break;
            };
            value = next;
        }
    }
    values.sort_unstable();
    values.dedup();
    if values.is_empty() {
        return Err("cron field contains no values".into());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use serde_json::json;

    fn ms(iso: &str) -> u64 {
        chrono::DateTime::parse_from_rfc3339(iso)
            .unwrap()
            .timestamp_millis() as u64
    }

    fn calendar(value: u64, offset_seconds: i32) -> Result<LocalMinute, String> {
        let time = FixedOffset::east_opt(offset_seconds)
            .unwrap()
            .timestamp_millis_opt(validate_timestamp(value)?)
            .single()
            .unwrap();
        Ok(LocalMinute {
            minute: time.minute(),
            hour: time.hour(),
            day: time.day(),
            month: time.month(),
            weekday: time.weekday().num_days_from_sunday(),
        })
    }

    fn utc(value: u64) -> Result<LocalMinute, String> {
        calendar(value, 0)
    }

    fn new_york_2026(value: u64) -> Result<LocalMinute, String> {
        let daylight = value >= ms("2026-03-08T07:00:00Z") && value < ms("2026-11-01T06:00:00Z");
        calendar(value, if daylight { -4 * 3600 } else { -5 * 3600 })
    }

    #[test]
    fn cron_supports_steps_ranges_lists_and_sunday_alias() {
        let cases = [
            (
                "*/30 * * * *",
                "2026-10-15T10:45:00Z",
                "2026-10-15T11:00:00Z",
            ),
            (
                "5,20 9-17/2 * * 1-5",
                "2026-10-15T09:20:00Z",
                "2026-10-15T11:05:00Z",
            ),
            ("0 8 * * 7", "2026-10-16T12:00:00Z", "2026-10-18T08:00:00Z"),
            (
                "0 8 * * 0,6",
                "2026-10-16T12:00:00Z",
                "2026-10-17T08:00:00Z",
            ),
        ];
        for (expression, from, expected) in cases {
            assert_eq!(
                next_run_with_local("cron", &json!(expression), ms(from), utc).unwrap(),
                Some(ms(expected)),
                "{expression}"
            );
        }
    }

    #[test]
    fn restricted_day_and_weekday_use_or() {
        let expression = json!("0 9 1 * 1");
        assert_eq!(
            next_run_with_local("cron", &expression, ms("2026-10-25T12:00:00Z"), utc).unwrap(),
            Some(ms("2026-10-26T09:00:00Z"))
        );
        assert_eq!(
            next_run_with_local("cron", &expression, ms("2026-10-31T12:00:00Z"), utc).unwrap(),
            Some(ms("2026-11-01T09:00:00Z"))
        );
    }

    #[test]
    fn cron_uses_local_calendar_but_returns_absolute_milliseconds() {
        let next = next_run_with_local(
            "cron",
            &json!("30 9 * * *"),
            ms("2026-10-15T00:00:00Z"),
            |value| calendar(value, 8 * 3600),
        )
        .unwrap();
        assert_eq!(next, Some(ms("2026-10-15T01:30:00Z")));
    }

    #[test]
    fn cron_always_advances_beyond_the_current_minute() {
        for from in ["2026-10-15T10:00:00Z", "2026-10-15T10:00:59.999Z"] {
            assert_eq!(
                next_run_with_local("cron", &json!("* * * * *"), ms(from), utc).unwrap(),
                Some(ms("2026-10-15T10:01:00Z"))
            );
        }
    }

    #[test]
    fn spring_forward_skips_missing_local_time() {
        let next = next_run_with_local(
            "cron",
            &json!("30 2 * * *"),
            ms("2026-03-08T06:59:30Z"),
            new_york_2026,
        )
        .unwrap();
        assert_eq!(next, Some(ms("2026-03-09T06:30:00Z")));
    }

    #[test]
    fn fall_back_preserves_two_distinct_instants_without_returning_the_past() {
        let expression = json!("30 1 * * *");
        let first = next_run_with_local(
            "cron",
            &expression,
            ms("2026-11-01T05:29:30Z"),
            new_york_2026,
        )
        .unwrap();
        let second = next_run_with_local(
            "cron",
            &expression,
            ms("2026-11-01T05:30:01Z"),
            new_york_2026,
        )
        .unwrap();
        let during_second_hour = next_run_with_local(
            "cron",
            &expression,
            ms("2026-11-01T06:29:30Z"),
            new_york_2026,
        )
        .unwrap();
        assert_eq!(first, Some(ms("2026-11-01T05:30:00Z")));
        assert_eq!(second, Some(ms("2026-11-01T06:30:00Z")));
        assert_eq!(during_second_hour, second);
        assert_ne!(first, second);
        assert_eq!(
            next_run_with_local(
                "cron",
                &json!("* * * * *"),
                ms("2026-11-01T05:59:30Z"),
                new_york_2026
            )
            .unwrap(),
            Some(ms("2026-11-01T06:00:00Z"))
        );
    }

    #[test]
    fn clock_rollback_never_makes_the_calculated_cursor_past_due() {
        for from in ["2026-10-15T10:34:40Z", "2026-10-15T08:02:40Z"] {
            let from = ms(from);
            let next = next_run_with_local("cron", &json!("*/15 * * * *"), from, utc)
                .unwrap()
                .unwrap();
            assert!(next > from);
        }
        // 是否已执行过相同绝对时点由持久触发收据裁定，纯算法不伪造去重状态。
    }

    #[test]
    fn yearly_search_is_bounded_to_366_days() {
        assert_eq!(
            next_run_with_local("cron", &json!("0 0 1 1 *"), ms("2026-01-01T00:00:00Z"), utc)
                .unwrap(),
            Some(ms("2027-01-01T00:00:00Z"))
        );
        assert_eq!(
            next_run_with_local(
                "cron",
                &json!("0 0 31 2 *"),
                ms("2026-01-01T00:00:00Z"),
                utc
            )
            .unwrap(),
            None
        );
        assert_eq!(
            next_run_with_local(
                "cron",
                &json!("0 0 29 2 *"),
                ms("2025-03-01T00:00:00Z"),
                utc
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn every_is_completion_based_with_a_one_minute_minimum() {
        let completed = ms("2026-10-15T10:42:00Z");
        for schedule in [json!(5000), json!("5000"), json!(0.5)] {
            assert_eq!(
                next_run("every", &schedule, completed).unwrap(),
                Some(completed + MIN_EVERY_INTERVAL_MS)
            );
        }
        let cursor = next_after_result("every", &json!(3_600_000), completed, true, 4).unwrap();
        assert_eq!(
            cursor,
            RunCursor {
                next_run_at_ms: Some(ms("2026-10-15T11:42:00Z")),
                consecutive_errors: 0,
                disable: false
            }
        );
    }

    #[test]
    fn completion_skips_missed_cron_slots_instead_of_replaying_them() {
        let cursor = next_after_result_with_local(
            "cron",
            &json!("*/10 * * * *"),
            ms("2026-10-15T10:46:00Z"),
            true,
            0,
            utc,
        )
        .unwrap();
        assert_eq!(cursor.next_run_at_ms, Some(ms("2026-10-15T10:50:00Z")));
    }

    #[test]
    fn failure_backoff_is_bounded_and_never_shortens_the_normal_interval() {
        let completed = ms("2026-10-15T10:00:00Z");
        for (previous, delay) in [
            (0, 60_000),
            (1, 300_000),
            (2, 900_000),
            (3, 3_600_000),
            (u32::MAX, 3_600_000),
        ] {
            let cursor =
                next_after_result("every", &json!(60_000), completed, false, previous).unwrap();
            assert_eq!(cursor.next_run_at_ms, Some(completed + delay));
            assert_eq!(cursor.consecutive_errors, previous.saturating_add(1));
        }
        assert_eq!(
            next_after_result("every", &json!(7_200_000), completed, false, 0)
                .unwrap()
                .next_run_at_ms,
            Some(completed + 7_200_000)
        );
    }

    #[test]
    fn one_time_jobs_disable_after_success_and_failure() {
        let schedule = json!("2026-10-15T10:00:00Z");
        let completed = ms("2026-10-15T10:01:00Z");
        assert_eq!(
            next_run("at", &schedule, completed - 120_000).unwrap(),
            Some(ms("2026-10-15T10:00:00Z"))
        );
        assert_eq!(
            next_after_result("at", &schedule, completed, true, 4).unwrap(),
            RunCursor {
                next_run_at_ms: None,
                consecutive_errors: 0,
                disable: true
            }
        );
        assert_eq!(
            next_after_result("at", &schedule, completed, false, 0).unwrap(),
            RunCursor {
                next_run_at_ms: Some(completed + 60_000),
                consecutive_errors: 1,
                disable: true
            }
        );
    }

    #[test]
    fn compatible_millisecond_prefixes_and_trailing_cron_fields_are_preserved() {
        assert_eq!(every_interval(&json!("  +120000ms")).unwrap(), 120_000);
        assert_eq!(
            next_run_with_local(
                "cron",
                &json!("0 9 * * * legacy"),
                ms("2026-10-15T08:00:00Z"),
                utc
            )
            .unwrap(),
            Some(ms("2026-10-15T09:00:00Z"))
        );
        assert_eq!(
            next_run("at", &json!("2026-10-15"), ms("2026-10-14T00:00:00Z")).unwrap(),
            Some(ms("2026-10-15T00:00:00Z"))
        );
        assert_eq!(
            next_run("at", &json!("2026-10-15T10:00:00+08:00"), 0).unwrap(),
            Some(ms("2026-10-15T02:00:00Z"))
        );
    }

    #[test]
    fn invalid_input_fails_explicitly_without_overflow_or_silent_fallback() {
        for expression in [
            "* * * *",
            "*/0 * * * *",
            "0 25 * * *",
            "0 9 * * 8",
            "0 9 * * 5-1",
            "0 9 * * 1-5/0",
        ] {
            assert!(
                next_run_with_local("cron", &json!(expression), 0, utc).is_err(),
                "{expression}"
            );
        }
        for schedule in [json!(0), json!(-1), json!("no"), Value::Null] {
            assert!(next_run("every", &schedule, 0).is_err());
        }
        assert!(next_run("unknown", &json!("* * * * *"), 0).is_err());
        assert!(next_run("at", &json!("not-a-date"), 0).is_err());
        assert!(next_run("cron", &json!("* * * * *"), u64::MAX).is_err());
        assert!(next_run_with_local("cron", &json!("* * * * *"), 0, |_| Err(
            "clock unavailable".into()
        ))
        .unwrap_err()
        .contains("clock unavailable"));
        assert!(
            next_run_with_local("cron", &json!("* * * * *"), 0, |_| Ok(LocalMinute {
                minute: 99,
                hour: 0,
                day: 1,
                month: 1,
                weekday: 0
            }))
            .is_err()
        );
    }
}
