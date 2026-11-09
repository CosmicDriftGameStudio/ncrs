//! What the answers of an own server used: one JSON line per answered request
//! in `usage.jsonl`, and the sums the usage page shows. Nothing here leaves
//! this computer, and nothing in a line says what a document contained.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Days, Local, NaiveDate, SecondsFormat, Utc};
use jsonc_parser::{parse_to_value, JsonObject, JsonValue, ParseOptions};

use crate::json::Json;

/// The file in the data folder of the app.
pub const USAGE_LOG_FILE: &str = "usage.jsonl";

/// Models beyond this many are drawn together as "Other" in the chart.
pub const CHART_MODELS: usize = 3;

/// Where the log is, next to the index.
pub fn usage_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join(USAGE_LOG_FILE)
}

/// What one answer says it used. `cost` only where the server reports a price.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Usage {
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// Of `tokens_in`, the part the server took from its cache.
    pub cached: u64,
    pub cost: Option<f64>,
}

/// The `usage` of a chat completions reply; what it does not say is zero. A
/// reply that is not JSON counts as a request that used nothing.
pub fn parse_usage(reply: &str) -> Usage {
    let Ok(Some(JsonValue::Object(root))) = parse_to_value(reply, &ParseOptions::default()) else {
        return Usage::default();
    };
    let Some(usage) = root.get_object("usage") else {
        return Usage::default();
    };
    Usage {
        tokens_in: count(usage, "prompt_tokens"),
        tokens_out: count(usage, "completion_tokens"),
        cached: usage
            .get_object("prompt_tokens_details")
            .map_or(0, |details| count(details, "cached_tokens")),
        cost: price(usage, "cost"),
    }
}

fn count(object: &JsonObject<'_>, name: &str) -> u64 {
    object.get_number(name).map_or(0, |text| {
        text.parse::<u64>().unwrap_or_else(|_not_whole| {
            text.parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map_or(0, |value| value.round() as u64)
        })
    })
}

fn price(object: &JsonObject<'_>, name: &str) -> Option<f64> {
    object
        .get_number(name)?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageRecord {
    pub time: DateTime<Utc>,
    pub model: String,
    pub usage: Usage,
}

impl UsageRecord {
    pub fn to_line(&self) -> String {
        let mut line = format!(
            "{{\"time\":{},\"model\":{},\"in\":{},\"out\":{},\"cached\":{}",
            Json::Text(self.time.to_rfc3339_opts(SecondsFormat::Secs, true)).pretty(),
            Json::text(&self.model).pretty(),
            self.usage.tokens_in,
            self.usage.tokens_out,
            self.usage.cached,
        );
        if let Some(cost) = self.usage.cost.filter(|cost| cost.is_finite()) {
            line.push_str(&format!(",\"cost\":{cost}"));
        }
        line.push('}');
        line
    }

    /// `None` for a line that is not one of ours; the rest of the log is read
    /// anyway.
    pub fn from_line(line: &str) -> Option<Self> {
        let Ok(Some(JsonValue::Object(root))) = parse_to_value(line, &ParseOptions::default())
        else {
            return None;
        };
        let time = DateTime::parse_from_rfc3339(root.get_string("time")?).ok()?;
        Some(Self {
            time: time.with_timezone(&Utc),
            model: root.get_string("model")?.to_string(),
            usage: Usage {
                tokens_in: count(&root, "in"),
                tokens_out: count(&root, "out"),
                cached: count(&root, "cached"),
                cost: price(&root, "cost"),
            },
        })
    }
}

/// Adds the line to the log. The caller ignores a failure: a request is
/// never lost for want of a line in the log.
pub fn append_usage(path: &Path, record: &UsageRecord) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut line = record.to_line();
    line.push('\n');
    file.write_all(line.as_bytes())
}

/// What a set of requests used.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Totals {
    pub requests: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cached: u64,
    /// Requests for which the server reported a price, and what they used.
    pub priced_requests: u64,
    pub priced_tokens: u64,
    pub cost: f64,
}

impl Totals {
    fn add(&mut self, usage: &Usage) {
        self.requests += 1;
        self.tokens_in += usage.tokens_in;
        self.tokens_out += usage.tokens_out;
        self.cached += usage.cached;
        if let Some(cost) = usage.cost {
            self.priced_requests += 1;
            self.priced_tokens += usage.tokens_in + usage.tokens_out;
            self.cost += cost;
        }
    }

    pub fn tokens(&self) -> u64 {
        self.tokens_in + self.tokens_out
    }

    /// The share of the tokens in that came from the cache.
    pub fn cache_hit_rate(&self) -> Option<f64> {
        (self.tokens_in > 0).then(|| self.cached as f64 / self.tokens_in as f64)
    }

    /// Money per million tokens, over the requests that have a price.
    pub fn cost_per_million_tokens(&self) -> Option<f64> {
        (self.priced_tokens > 0).then(|| self.cost / self.priced_tokens as f64 * 1_000_000.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelUsage {
    pub model: String,
    pub totals: Totals,
}

/// The tokens of one day, per chart series: the top models in the order of
/// [`UsageSummary::chart_models`], then everything else.
#[derive(Debug, Clone, PartialEq)]
pub struct DayUsage {
    pub date: NaiveDate,
    pub tokens: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageSummary {
    pub totals: Totals,
    /// Every model of the period, the one with the most tokens first.
    pub by_model: Vec<ModelUsage>,
    /// The models with a series of their own in the chart.
    pub chart_models: Vec<String>,
    /// Every day of the period, the oldest first, those without a request too.
    pub days: Vec<DayUsage>,
}

/// The last `days` days up to and including the day of `now`, from the lines
/// of the log, by local date. A line that cannot be read is skipped.
pub fn summarize<'a>(
    lines: impl IntoIterator<Item = &'a str>,
    days: u32,
    now: DateTime<Local>,
) -> UsageSummary {
    let today = now.date_naive();
    let first = today
        .checked_sub_days(Days::new(u64::from(days.saturating_sub(1))))
        .unwrap_or(today);
    let records: Vec<UsageRecord> = lines
        .into_iter()
        .filter_map(UsageRecord::from_line)
        .filter(|record| (first..=today).contains(&record.time.with_timezone(&Local).date_naive()))
        .collect();

    let mut totals = Totals::default();
    let mut per_model: BTreeMap<&str, Totals> = BTreeMap::new();
    for record in &records {
        totals.add(&record.usage);
        per_model
            .entry(record.model.as_str())
            .or_default()
            .add(&record.usage);
    }
    let mut by_model: Vec<ModelUsage> = per_model
        .into_iter()
        .map(|(model, model_totals)| ModelUsage {
            model: model.to_owned(),
            totals: model_totals,
        })
        .collect();
    by_model.sort_by(|left, right| {
        right
            .totals
            .tokens()
            .cmp(&left.totals.tokens())
            .then_with(|| left.model.cmp(&right.model))
    });
    let chart_models: Vec<String> = by_model
        .iter()
        .take(CHART_MODELS)
        .map(|usage| usage.model.clone())
        .collect();

    let mut per_day: BTreeMap<NaiveDate, Vec<u64>> = BTreeMap::new();
    let mut date = first;
    while date <= today {
        per_day.insert(date, vec![0; chart_models.len() + 1]);
        match date.checked_add_days(Days::new(1)) {
            Some(next) => date = next,
            None => break,
        }
    }
    for record in &records {
        let series = chart_models
            .iter()
            .position(|model| *model == record.model)
            .unwrap_or(chart_models.len());
        if let Some(slot) = per_day
            .get_mut(&record.time.with_timezone(&Local).date_naive())
            .and_then(|tokens| tokens.get_mut(series))
        {
            *slot += record.usage.tokens_in + record.usage.tokens_out;
        }
    }
    UsageSummary {
        totals,
        by_model,
        chart_models,
        days: per_day
            .into_iter()
            .map(|(day, tokens)| DayUsage { date: day, tokens })
            .collect(),
    }
}

/// The lines of the log, for [`summarize`]; none when there is no log yet.
pub fn read_usage_log(path: &Path) -> String {
    // ponytail: whole file is read on page open; fine up to ~100k requests, move into the index if it ever is not.
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The sums for each of `periods` (in days) as of now. Without a log they
/// are all empty.
pub fn usage_summaries(log: Option<&Path>, periods: &[u32]) -> Vec<(u32, UsageSummary)> {
    let text = log.map(read_usage_log).unwrap_or_default();
    let now = Local::now();
    periods
        .iter()
        .map(|&days| (days, summarize(text.lines(), days, now)))
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    fn at(day: u32, hour: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, day, hour, 30, 0).unwrap()
    }

    fn record(day: u32, model: &str, tokens_in: u64, tokens_out: u64, cost: Option<f64>) -> String {
        UsageRecord {
            time: at(day, 9).with_timezone(&Utc),
            model: model.to_owned(),
            usage: Usage {
                tokens_in,
                tokens_out,
                cached: tokens_in.div_euclid(2),
                cost,
            },
        }
        .to_line()
    }

    #[test]
    fn a_full_usage_object_is_read() {
        let usage = parse_usage(
            r#"{"choices": [], "usage": {"prompt_tokens": 120, "completion_tokens": 30,
                "prompt_tokens_details": {"cached_tokens": 100}, "cost": 0.0021}}"#,
        );

        assert_eq!(
            usage,
            Usage {
                tokens_in: 120,
                tokens_out: 30,
                cached: 100,
                cost: Some(0.0021),
            }
        );
    }

    #[test]
    fn missing_fields_are_zero_and_no_price() {
        let usage = parse_usage(r#"{"usage": {"prompt_tokens": 7}}"#);

        assert_eq!(
            usage,
            Usage {
                tokens_in: 7,
                ..Usage::default()
            }
        );
        assert_eq!(parse_usage(r#"{"choices": []}"#), Usage::default());
        assert_eq!(parse_usage("not json"), Usage::default());
    }

    #[test]
    fn a_price_is_read_only_when_it_is_a_number() {
        assert_eq!(
            parse_usage(r#"{"usage": {"cost": 0}}"#).cost,
            Some(0.0),
            "free is a price"
        );
        assert_eq!(parse_usage(r#"{"usage": {"cost": "cheap"}}"#).cost, None);
        assert_eq!(parse_usage(r#"{"usage": {"cost": -1}}"#).cost, None);
    }

    #[test]
    fn a_line_reads_back_the_same_and_keeps_odd_names_apart() {
        let original = UsageRecord {
            time: at(9, 12).with_timezone(&Utc),
            model: "vendor/model \"x\"\n".to_owned(),
            usage: Usage {
                tokens_in: 5,
                tokens_out: 6,
                cached: 2,
                cost: Some(0.5),
            },
        };

        let line = original.to_line();

        assert!(!line.contains('\n'));
        assert_eq!(UsageRecord::from_line(&line), Some(original));
        assert_eq!(UsageRecord::from_line("{\"time\": \"nonsense\"}"), None);
        assert_eq!(UsageRecord::from_line(""), None);
    }

    #[test]
    fn appended_lines_end_up_one_per_request() {
        let dir = tempfile::tempdir().unwrap();
        let path = usage_log_path(dir.path());
        let first = UsageRecord::from_line(&record(8, "a", 10, 5, None)).unwrap();

        append_usage(&path, &first).unwrap();
        append_usage(&path, &first).unwrap();

        assert_eq!(read_usage_log(&path).lines().count(), 2);
        assert_eq!(read_usage_log(&dir.path().join("missing")), "");
    }

    #[test]
    fn totals_cover_the_period_and_skip_what_cannot_be_read() {
        let lines = [
            record(9, "a", 100, 20, Some(0.10)),
            record(8, "a", 100, 20, None),
            record(1, "a", 1_000, 0, None),
            "garbage".to_owned(),
            record(9, "b", 10, 10, Some(0.05)),
        ];

        let summary = summarize(lines.iter().map(String::as_str), 7, at(9, 20));

        assert_eq!(summary.totals.requests, 3);
        assert_eq!(summary.totals.tokens(), 260);
        assert_eq!(summary.totals.priced_requests, 2);
        assert!((summary.totals.cost - 0.15).abs() < 1e-9);
        assert_eq!(summary.totals.cache_hit_rate(), Some(105.0 / 210.0));
        let per_million = summary.totals.cost_per_million_tokens().unwrap();
        assert!((per_million - 0.15 / 140.0 * 1_000_000.0).abs() < 1e-6);
        assert_eq!(summary.days.len(), 7);
        assert_eq!(summary.days[0].date, at(3, 12).date_naive());
        assert_eq!(summary.days[6].date, at(9, 12).date_naive());
    }

    #[test]
    fn a_longer_period_takes_in_the_older_line() {
        let lines = [record(1, "a", 1_000, 0, None), record(9, "a", 1, 1, None)];

        let week = summarize(lines.iter().map(String::as_str), 7, at(9, 20));
        let month = summarize(lines.iter().map(String::as_str), 30, at(9, 20));

        assert_eq!(week.totals.requests, 1);
        assert_eq!(month.totals.requests, 2);
        assert_eq!(month.days.len(), 30);
    }

    #[test]
    fn models_are_ordered_by_tokens_and_the_rest_folds_into_other() {
        let lines = [
            record(9, "small", 1, 0, None),
            record(9, "d", 40, 0, None),
            record(9, "c", 30, 0, None),
            record(9, "b", 20, 0, None),
            record(9, "a", 10, 0, None),
            record(8, "d", 4, 0, None),
        ];

        let summary = summarize(lines.iter().map(String::as_str), 7, at(9, 20));

        let order: Vec<&str> = summary
            .by_model
            .iter()
            .map(|usage| usage.model.as_str())
            .collect();
        assert_eq!(order, ["d", "c", "b", "a", "small"]);
        assert_eq!(summary.chart_models, ["d", "c", "b"]);
        let today = summary.days.last().unwrap();
        assert_eq!(today.tokens, [40, 30, 20, 11]);
        let yesterday = &summary.days[5];
        assert_eq!(yesterday.tokens, [4, 0, 0, 0]);
    }

    #[test]
    fn an_empty_log_has_no_rates_and_still_has_its_days() {
        let summary = summarize([], 30, at(9, 20));

        assert_eq!(summary.totals, Totals::default());
        assert_eq!(summary.totals.cache_hit_rate(), None);
        assert_eq!(summary.totals.cost_per_million_tokens(), None);
        assert!(summary.by_model.is_empty());
        assert_eq!(summary.days.len(), 30);
    }
}
