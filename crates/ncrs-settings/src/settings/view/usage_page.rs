//! The Usage page: what Hermes used on the own server, from the local log.

use iced::widget::{container, row, Column};
use iced::{Element, Length};
use ncrs_ui_kit::kit;

use super::*;
use ncrs_ai::{Totals, UsageSummary, CHART_MODELS};
use ncrs_i18n::fill_template;
use ncrs_ui_kit::day_bars::{day_bars, BarPart, LegendEntry};
use ncrs_ui_kit::kit::Figure;
use ncrs_ui_kit::meter::{meter, Meter, MeterLevel};
use ncrs_ui_kit::setting_rows::{simple_row, Group};
use ncrs_ui_kit::theme::spacing;

/// Tokens as people read them: 980, 12.4k, 3.2M.
fn tokens_text(tokens: u64) -> String {
    match tokens {
        0..=9_999 => tokens.to_string(),
        10_000..=999_999 => format!("{:.1}k", tokens as f64 / 1_000.0),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}

fn money_text(cost: f64) -> String {
    if cost > 0.0 && cost < 0.01 {
        format!("${cost:.4}")
    } else {
        format!("${cost:.2}")
    }
}

fn percent_text(rate: Option<f64>) -> String {
    format!("{:.0}", rate.unwrap_or(0.0) * 100.0)
}

fn money_figure(cost: Option<f64>, view: &SettingsView<'_>) -> Figure {
    match cost {
        Some(amount) => Figure::Value(money_text(amount)),
        None => Figure::Note(view.lang.text(Msg::SettingsUsageNotReported).to_owned()),
    }
}

fn overview<'a>(summary: &UsageSummary, view: &SettingsView<'a>) -> Element<'a, Message> {
    let (lang, palette) = (view.lang, view.palette);
    let totals = &summary.totals;
    let labels: Vec<String> = USAGE_PERIODS
        .iter()
        .map(|days| {
            fill_template(
                lang.text(Msg::SettingsUsagePeriodDays),
                &[("{days}", &days.to_string())],
            )
        })
        .collect();
    let options: Vec<(&str, Message)> = labels
        .iter()
        .zip(USAGE_PERIODS)
        .map(|(label, days)| (label.as_str(), SettingsMessage::UsagePeriod(days)))
        .collect();
    let chosen = USAGE_PERIODS
        .iter()
        .position(|&days| days == view.state.usage_days)
        .unwrap_or_default();
    let priced = totals.priced_requests > 0;
    let figures = [
        (
            lang.text(Msg::SettingsUsageSpend),
            money_figure(priced.then_some(totals.cost), view),
        ),
        (
            lang.text(Msg::SettingsUsageRequests),
            Figure::Value(totals.requests.to_string()),
        ),
        (
            lang.text(Msg::SettingsUsageTokens),
            Figure::Value(tokens_text(totals.tokens())),
        ),
        (
            lang.text(Msg::SettingsUsageCacheRate),
            Figure::Value(format!("{} %", percent_text(totals.cache_hit_rate()))),
        ),
        (
            lang.text(Msg::SettingsUsageCostPerMillion),
            money_figure(totals.cost_per_million_tokens(), view),
        ),
    ];
    let priced_line = fill_template(
        lang.text(Msg::SettingsUsagePricedLine),
        &[
            ("{priced}", &totals.priced_requests.to_string()),
            ("{total}", &totals.requests.to_string()),
        ],
    );
    let period_row = simple_row(
        Msg::SettingsUsagePeriod,
        None,
        kit::segmented(&options, chosen, true, palette),
        lang,
    );
    let period = ncrs_ui_kit::setting_rows::sections(
        vec![Group {
            intro: None,
            title: String::new(),
            rows: vec![period_row],
        }],
        |_| true,
        None,
        palette,
    );
    Column::with_children(period)
        .push(kit::figure_row(&figures, palette))
        .push(kit::dim(priced_line, palette))
        .spacing(spacing::M)
        .into()
}

fn per_day<'a>(summary: &UsageSummary, view: &SettingsView<'a>) -> Element<'a, Message> {
    let (lang, palette) = (view.lang, view.palette);
    let other_series = CHART_MODELS;
    let series_of = |index: usize| {
        if index < summary.chart_models.len() {
            index
        } else {
            other_series
        }
    };
    let bars: Vec<Vec<BarPart>> = summary
        .days
        .iter()
        .map(|day| {
            day.tokens
                .iter()
                .enumerate()
                .map(|(index, &portion)| BarPart {
                    portion,
                    series: series_of(index),
                })
                .collect()
        })
        .collect();
    let other_used = summary.days.iter().any(|day| {
        day.tokens
            .iter()
            .skip(summary.chart_models.len())
            .any(|&t| t > 0)
    });
    let other_name = lang.text(Msg::SettingsUsageOther);
    let mut legend: Vec<LegendEntry<'_>> = summary
        .chart_models
        .iter()
        .enumerate()
        .map(|(series, name)| LegendEntry { name, series })
        .collect();
    if other_used {
        legend.push(LegendEntry {
            name: other_name,
            series: other_series,
        });
    }
    let mut chart = Column::new().push(day_bars(&bars, &legend, palette));
    let busiest = summary
        .days
        .iter()
        .max_by_key(|day| day.tokens.iter().sum::<u64>());
    if let (Some(first), Some(last), Some(peak)) =
        (summary.days.first(), summary.days.last(), busiest)
    {
        let peak_text = fill_template(
            lang.text(Msg::SettingsUsagePeak),
            &[
                ("{tokens}", &tokens_text(peak.tokens.iter().sum())),
                ("{day}", &peak.date.to_string()),
            ],
        );
        chart = chart.push(
            row![
                kit::dim(first.date.to_string(), palette),
                container("").width(Length::Fill),
                kit::dim(peak_text, palette),
                container("").width(Length::Fill),
                kit::dim(last.date.to_string(), palette),
            ]
            .spacing(spacing::M),
        );
    }
    chart.spacing(spacing::XS).into()
}

fn model_detail(totals: &Totals, view: &SettingsView<'_>) -> String {
    let message = match (totals.priced_requests > 0, totals.requests == 1) {
        (true, false) => Msg::SettingsUsageModelDetail,
        (false, false) => Msg::SettingsUsageModelDetailNoPrice,
        (true, true) => Msg::SettingsUsageModelDetailOne,
        (false, true) => Msg::SettingsUsageModelDetailNoPriceOne,
    };
    fill_template(
        view.lang.text(message),
        &[
            ("{requests}", &totals.requests.to_string()),
            ("{percent}", &percent_text(totals.cache_hit_rate())),
            ("{cost}", &money_text(totals.cost)),
        ],
    )
}

fn by_model<'a>(summary: &UsageSummary, view: &SettingsView<'a>) -> Element<'a, Message> {
    let all = summary.totals.tokens().max(1);
    let meters = summary.by_model.iter().map(|model| {
        let detail = model_detail(&model.totals, view);
        let tokens = model.totals.tokens();
        meter(
            Meter {
                title: &model.model,
                value: &tokens_text(tokens),
                fraction: tokens as f32 / all as f32,
                level: MeterLevel::Normal,
                detail: &detail,
            },
            view.palette,
        )
    });
    Column::with_children(meters).spacing(spacing::M).into()
}

/// The sections under the heading of the page.
pub fn usage_sections<'a>(view: &SettingsView<'a>) -> Vec<Element<'a, Message>> {
    let (lang, palette) = (view.lang, view.palette);
    let Some(summary) = view.state.usage_summary() else {
        return Vec::new();
    };
    let empty = view.state.usage.as_ref().is_some_and(|all| {
        all.iter()
            .all(|(_, per_period)| per_period.totals.requests == 0)
    });
    if empty {
        return vec![kit::dim(
            lang.text(Msg::SettingsUsageNone).to_owned(),
            palette,
        )];
    }
    vec![
        kit::first_section(
            lang.text(Msg::SettingsUsageOverview),
            overview(summary, view),
            palette,
        ),
        kit::section(
            lang.text(Msg::SettingsUsagePerDay),
            per_day(summary, view),
            palette,
        ),
        kit::section(
            lang.text(Msg::SettingsUsageByModel),
            by_model(summary, view),
            palette,
        ),
    ]
}
