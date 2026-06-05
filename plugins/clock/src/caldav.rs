use std::{
    fs::{OpenOptions, remove_file},
    io::Write,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use std::os::unix::fs::OpenOptionsExt;

use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, Weekday};
use oxibar_plugin_api::toml::Table;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarEvent {
    pub date: NaiveDate,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CaldavConfig {
    pub url: String,
    pub username: String,
    pub password_env: String,
    pub refresh_minutes: u64,
    pub days_ahead: i64,
}

pub(crate) fn read_config(clock: Option<&Table>) -> Option<CaldavConfig> {
    let caldav = clock?.get("caldav").and_then(|value| value.as_table())?;
    if caldav.get("enabled").and_then(|value| value.as_bool()) == Some(false) {
        return None;
    }

    let url = caldav.get("url")?.as_str()?.trim().to_owned();
    let username = caldav.get("username")?.as_str()?.trim().to_owned();
    let password_env = caldav
        .get("password_env")
        .and_then(|value| value.as_str())
        .unwrap_or("OXIBAR_CALDAV_PASSWORD")
        .trim()
        .to_owned();
    if url.is_empty() || username.is_empty() || password_env.is_empty() {
        return None;
    }

    let refresh_minutes = caldav
        .get("refresh_minutes")
        .and_then(|value| value.as_integer())
        .filter(|value| *value > 0)
        .map(|value| value as u64)
        .unwrap_or(15);
    let days_ahead = caldav
        .get("days_ahead")
        .and_then(|value| value.as_integer())
        .filter(|value| *value > 0)
        .unwrap_or(30);

    Some(CaldavConfig {
        url,
        username,
        password_env,
        refresh_minutes,
        days_ahead,
    })
}

pub(crate) fn load_events(config: CaldavConfig) -> Result<Vec<CalendarEvent>, String> {
    if !config.url.starts_with("https://") {
        return Err("clock: CalDAV url must use https://".to_owned());
    }
    let password = std::env::var(&config.password_env)
        .map_err(|_| format!("clock: env var {} is not set", config.password_env))?;
    let curl_config = write_curl_config(&config.username, &password)?;
    let today = Local::now().date_naive();
    let start =
        NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("valid current month start");
    let end = start + ChronoDuration::days(config.days_ahead);
    let body = calendar_query_body(start, end);
    let curl_config_arg = curl_config.to_string_lossy().into_owned();
    let output = Command::new("curl")
        .args([
            "-fsS",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--config",
            &curl_config_arg,
            "-X",
            "REPORT",
            "-H",
            "Depth: 1",
            "-H",
            "Content-Type: application/xml; charset=utf-8",
            "--data-binary",
            &body,
            &config.url,
        ])
        .output();
    let _ = remove_file(&curl_config);
    let output =
        output.map_err(|error| format!("clock: failed to run curl for CalDAV sync: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return if stderr.is_empty() {
            Err("clock: CalDAV sync failed".to_owned())
        } else {
            Err(format!("clock: CalDAV sync failed: {stderr}"))
        };
    }

    let response = String::from_utf8_lossy(&output.stdout);
    Ok(parse_caldav_response_in_range(&response, start, end))
}

fn write_curl_config(username: &str, password: &str) -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let path = std::env::temp_dir().join(format!(
        "oxibar-caldav-{}-{nanos}.curlrc",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| format!("clock: failed to create curl config: {error}"))?;
    writeln!(
        file,
        "user = \"{}:{}\"",
        curl_config_escape(username),
        curl_config_escape(password)
    )
    .map_err(|error| format!("clock: failed to write curl config: {error}"))?;
    Ok(path)
}

fn curl_config_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], "")
}

fn calendar_query_body(start: NaiveDate, end: NaiveDate) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <c:calendar-data />
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{}T000000Z" end="{}T235959Z" />
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#,
        date_token(start),
        date_token(end)
    )
}

fn date_token(date: NaiveDate) -> String {
    format!("{:04}{:02}{:02}", date.year(), date.month(), date.day())
}

#[cfg(test)]
pub(crate) fn parse_caldav_response(response: &str) -> Vec<CalendarEvent> {
    parse_caldav_response_in_range(
        response,
        NaiveDate::MIN.succ_opt().unwrap_or(NaiveDate::MIN),
        NaiveDate::MAX.pred_opt().unwrap_or(NaiveDate::MAX),
    )
}

pub(crate) fn parse_caldav_response_in_range(
    response: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Vec<CalendarEvent> {
    let mut events = Vec::new();
    let mut rest = response;
    while let Some(tag_start) = rest.find("calendar-data") {
        let after_name = &rest[tag_start + "calendar-data".len()..];
        let Some(tag_end) = after_name.find('>') else {
            break;
        };
        let data_start = tag_start + "calendar-data".len() + tag_end + 1;
        let after_data = &rest[data_start..];
        let Some(data_end) = after_data.find("</") else {
            break;
        };
        let ics = xml_unescape(strip_cdata(&after_data[..data_end]));
        events.extend(parse_ics_events_in_range(&ics, start, end));
        let Some(close_end) = after_data[data_end..].find('>') else {
            break;
        };
        rest = &after_data[data_end + close_end + 1..];
    }
    events.sort_by_key(|event| (event.date, event.summary.clone()));
    events.dedup_by(|a, b| a.date == b.date && a.summary == b.summary);
    events
}

#[cfg(test)]
pub(crate) fn parse_ics_events(ics: &str) -> Vec<CalendarEvent> {
    parse_ics_events_in_range(
        ics,
        NaiveDate::MIN.succ_opt().unwrap_or(NaiveDate::MIN),
        NaiveDate::MAX.pred_opt().unwrap_or(NaiveDate::MAX),
    )
}

pub(crate) fn parse_ics_events_in_range(
    ics: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Vec<CalendarEvent> {
    let lines = unfold_ics_lines(ics);
    let mut events = Vec::new();
    let mut in_event = false;
    let mut raw = RawEvent::default();

    for line in lines {
        match line.as_str() {
            "BEGIN:VEVENT" => {
                in_event = true;
                raw = RawEvent::default();
            }
            "END:VEVENT" if in_event => {
                events.extend(expand_event(&raw, start, end));
                in_event = false;
            }
            _ if in_event => {
                if let Some(value) = property_value(&line, "DTSTART") {
                    raw.start = parse_ics_date(value);
                } else if let Some(value) = property_value(&line, "SUMMARY") {
                    raw.summary = Some(unescape_ics_text(value));
                } else if let Some(value) = property_value(&line, "DESCRIPTION") {
                    raw.description = non_empty(unescape_ics_text(value));
                } else if let Some(value) = property_value(&line, "LOCATION") {
                    raw.location = non_empty(unescape_ics_text(value));
                } else if let Some(value) = property_value(&line, "RRULE") {
                    raw.rrule = parse_rrule(value);
                } else if let Some(value) = property_value(&line, "RDATE") {
                    raw.rdates.extend(parse_ics_date_list(value));
                } else if let Some(value) = property_value(&line, "EXDATE") {
                    raw.exdates.extend(parse_ics_date_list(value));
                }
            }
            _ => {}
        }
    }

    events.sort_by_key(|event| (event.date, event.summary.clone()));
    events.dedup_by(|a, b| a.date == b.date && a.summary == b.summary);
    events
}

#[derive(Default)]
struct RawEvent {
    start: Option<NaiveDate>,
    summary: Option<String>,
    description: Option<String>,
    location: Option<String>,
    rrule: Option<RRule>,
    rdates: Vec<NaiveDate>,
    exdates: Vec<NaiveDate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RRule {
    freq: Frequency,
    interval: i32,
    until: Option<NaiveDate>,
    count: Option<usize>,
    byday: Vec<Weekday>,
    bymonthday: Vec<u32>,
}

fn expand_event(
    raw: &RawEvent,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Vec<CalendarEvent> {
    let Some(start) = raw.start else {
        return Vec::new();
    };
    let Some(summary) = raw.summary.as_ref() else {
        return Vec::new();
    };

    let mut dates = Vec::new();
    if let Some(rrule) = raw.rrule.as_ref() {
        dates.extend(expand_rrule(start, rrule, window_start, window_end));
    } else if in_window(start, window_start, window_end) {
        dates.push(start);
    }

    dates.extend(
        raw.rdates
            .iter()
            .copied()
            .filter(|date| in_window(*date, window_start, window_end)),
    );

    dates.retain(|date| !raw.exdates.contains(date));
    dates.sort_unstable();
    dates.dedup();

    dates
        .into_iter()
        .map(|date| CalendarEvent {
            date,
            summary: summary.clone(),
            description: raw.description.clone(),
            location: raw.location.clone(),
        })
        .collect()
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn expand_rrule(
    start: NaiveDate,
    rrule: &RRule,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Vec<NaiveDate> {
    match rrule.freq {
        Frequency::Daily => expand_stepped(start, rrule, window_start, window_end, |date, step| {
            date + ChronoDuration::days(step as i64)
        }),
        Frequency::Weekly if !rrule.byday.is_empty() => {
            expand_weekly_byday(start, rrule, window_start, window_end)
        }
        Frequency::Weekly => {
            expand_stepped(start, rrule, window_start, window_end, |date, step| {
                date + ChronoDuration::weeks(step as i64)
            })
        }
        Frequency::Monthly => expand_monthly(start, rrule, window_start, window_end),
        Frequency::Yearly => expand_yearly(start, rrule, window_start, window_end),
    }
}

fn expand_stepped(
    start: NaiveDate,
    rrule: &RRule,
    window_start: NaiveDate,
    window_end: NaiveDate,
    next: impl Fn(NaiveDate, i32) -> NaiveDate,
) -> Vec<NaiveDate> {
    let mut dates = Vec::new();
    let mut date = start;
    let mut generated = 0usize;
    let interval = rrule.interval.max(1);
    while !after_limit(date, rrule, window_end, generated) && generated < 10_000 {
        generated += 1;
        if in_window(date, window_start, window_end) {
            dates.push(date);
        }
        date = next(date, interval);
    }
    dates
}

fn expand_weekly_byday(
    start: NaiveDate,
    rrule: &RRule,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Vec<NaiveDate> {
    let mut dates = Vec::new();
    let mut date = start;
    let mut generated = 0usize;
    let interval = rrule.interval.max(1) as i64;
    while !after_limit(date, rrule, window_end, generated) && generated < 20_000 {
        if date >= start
            && rrule.byday.contains(&date.weekday())
            && ((date - start).num_days().div_euclid(7) % interval == 0)
        {
            generated += 1;
            if in_window(date, window_start, window_end) {
                dates.push(date);
            }
        }
        date += ChronoDuration::days(1);
    }
    dates
}

fn expand_monthly(
    start: NaiveDate,
    rrule: &RRule,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Vec<NaiveDate> {
    let mut dates = Vec::new();
    let mut month_offset = 0i32;
    let mut generated = 0usize;
    let days = if rrule.bymonthday.is_empty() {
        vec![start.day()]
    } else {
        rrule.bymonthday.clone()
    };

    while generated < 10_000 {
        let month_start = add_months(month_start(start), month_offset);
        for day in &days {
            let Some(date) = NaiveDate::from_ymd_opt(month_start.year(), month_start.month(), *day)
            else {
                continue;
            };
            if date < start {
                continue;
            }
            if after_limit(date, rrule, window_end, generated) {
                return dates;
            }
            generated += 1;
            if in_window(date, window_start, window_end) {
                dates.push(date);
            }
        }
        month_offset += rrule.interval.max(1);
    }
    dates
}

fn expand_yearly(
    start: NaiveDate,
    rrule: &RRule,
    window_start: NaiveDate,
    window_end: NaiveDate,
) -> Vec<NaiveDate> {
    let mut dates = Vec::new();
    let mut year = start.year();
    let mut generated = 0usize;
    let interval = rrule.interval.max(1);
    while generated < 10_000 {
        let Some(date) = NaiveDate::from_ymd_opt(year, start.month(), start.day()) else {
            year += interval;
            continue;
        };
        if after_limit(date, rrule, window_end, generated) {
            break;
        }
        generated += 1;
        if in_window(date, window_start, window_end) {
            dates.push(date);
        }
        year += interval;
    }
    dates
}

fn after_limit(date: NaiveDate, rrule: &RRule, window_end: NaiveDate, generated: usize) -> bool {
    date > window_end
        || rrule.until.is_some_and(|until| date > until)
        || rrule.count.is_some_and(|count| generated >= count)
}

fn in_window(date: NaiveDate, start: NaiveDate, end: NaiveDate) -> bool {
    date >= start && date <= end
}

fn parse_rrule(value: &str) -> Option<RRule> {
    let mut freq = None;
    let mut interval = 1;
    let mut until = None;
    let mut count = None;
    let mut byday = Vec::new();
    let mut bymonthday = Vec::new();

    for part in value.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "FREQ" => {
                freq = match value {
                    "DAILY" => Some(Frequency::Daily),
                    "WEEKLY" => Some(Frequency::Weekly),
                    "MONTHLY" => Some(Frequency::Monthly),
                    "YEARLY" => Some(Frequency::Yearly),
                    _ => None,
                };
            }
            "INTERVAL" => interval = value.parse::<i32>().unwrap_or(1).max(1),
            "UNTIL" => until = parse_ics_date(value),
            "COUNT" => count = value.parse().ok(),
            "BYDAY" => byday = value.split(',').filter_map(parse_weekday).collect(),
            "BYMONTHDAY" => {
                bymonthday = value
                    .split(',')
                    .filter_map(|day| day.parse::<u32>().ok())
                    .filter(|day| (1..=31).contains(day))
                    .collect()
            }
            _ => {}
        }
    }

    Some(RRule {
        freq: freq?,
        interval,
        until,
        count,
        byday,
        bymonthday,
    })
}

fn parse_weekday(value: &str) -> Option<Weekday> {
    let value = value.trim_start_matches(|c: char| c == '+' || c == '-' || c.is_ascii_digit());
    match value {
        "MO" => Some(Weekday::Mon),
        "TU" => Some(Weekday::Tue),
        "WE" => Some(Weekday::Wed),
        "TH" => Some(Weekday::Thu),
        "FR" => Some(Weekday::Fri),
        "SA" => Some(Weekday::Sat),
        "SU" => Some(Weekday::Sun),
        _ => None,
    }
}

fn parse_ics_date_list(value: &str) -> Vec<NaiveDate> {
    value.split(',').filter_map(parse_ics_date).collect()
}

fn month_start(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("valid month start")
}

fn add_months(date: NaiveDate, delta: i32) -> NaiveDate {
    let zero_based = date.year() * 12 + date.month0() as i32 + delta;
    let year = zero_based.div_euclid(12);
    let month = zero_based.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(year, month, 1).expect("valid shifted year/month")
}

fn unfold_ics_lines(ics: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in ics.replace("\r\n", "\n").lines() {
        if raw.starts_with(' ') || raw.starts_with('\t') {
            if let Some(last) = lines.last_mut() {
                last.push_str(raw.trim_start());
            }
        } else {
            lines.push(raw.trim_end().to_owned());
        }
    }
    lines
}

fn property_value<'a>(line: &'a str, property: &str) -> Option<&'a str> {
    let (name, value) = line.split_once(':')?;
    if name.split(';').next()? == property {
        Some(value)
    } else {
        None
    }
}

fn parse_ics_date(value: &str) -> Option<NaiveDate> {
    let date = value.get(..8)?;
    NaiveDate::parse_from_str(date, "%Y%m%d").ok()
}

fn unescape_ics_text(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&#13;", "")
        .replace("&#10;", "\n")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn strip_cdata(value: &str) -> &str {
    value
        .strip_prefix("<![CDATA[")
        .and_then(|value| value.strip_suffix("]]>"))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ics_events() {
        let events = parse_ics_events(
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20260605\nSUMMARY:Meet\\, test\nEND:VEVENT\nEND:VCALENDAR",
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].date, NaiveDate::from_ymd_opt(2026, 6, 5).unwrap());
        assert_eq!(events[0].summary, "Meet, test");
    }

    #[test]
    fn parses_event_details_for_tooltips() {
        let events = parse_ics_events(
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20260605\nSUMMARY:Meet\nLOCATION:Office\nDESCRIPTION:Discuss roadmap\\nBring notes\nEND:VEVENT\nEND:VCALENDAR",
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].location.as_deref(), Some("Office"));
        assert_eq!(
            events[0].description.as_deref(),
            Some("Discuss roadmap\nBring notes")
        );
    }

    #[test]
    fn parses_caldav_calendar_data() {
        let response = "<d:multistatus><cal:calendar-data>BEGIN:VCALENDAR&#10;BEGIN:VEVENT&#10;DTSTART:20260605T120000Z&#10;SUMMARY:Call&amp;amp;Chat&#10;END:VEVENT&#10;END:VCALENDAR</cal:calendar-data></d:multistatus>";
        let events = parse_caldav_response(response);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "Call&amp;Chat");
    }

    #[test]
    fn expands_recurring_events_into_requested_window() {
        let events = parse_ics_events_in_range(
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20260501\nRRULE:FREQ=WEEKLY;BYDAY=FR;COUNT=10\nSUMMARY:Weekly sync\nEND:VEVENT\nEND:VCALENDAR",
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        );

        assert_eq!(events.len(), 4);
        assert!(events.iter().all(|event| event.date.month() == 6));
    }

    #[test]
    fn parses_caldav_cdata_and_rdate_occurrences() {
        let response = "<d:multistatus><cal:calendar-data><![CDATA[BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20260501\nRDATE;VALUE=DATE:20260607,20260608\nSUMMARY:Extra dates\nEND:VEVENT\nEND:VCALENDAR]]></cal:calendar-data></d:multistatus>";

        let events = parse_caldav_response_in_range(
            response,
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        );

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].date, NaiveDate::from_ymd_opt(2026, 6, 7).unwrap());
        assert_eq!(events[1].date, NaiveDate::from_ymd_opt(2026, 6, 8).unwrap());
    }

    #[test]
    fn load_events_rejects_unencrypted_urls_before_secret_lookup() {
        let config = CaldavConfig {
            url: "http://example.test/calendar".to_owned(),
            username: "user".to_owned(),
            password_env: "SHOULD_NOT_BE_NEEDED".to_owned(),
            refresh_minutes: 15,
            days_ahead: 30,
        };

        let error = load_events(config).unwrap_err();

        assert!(error.contains("https://"));
    }
}
