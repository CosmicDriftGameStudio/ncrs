//! What the run reads off the chat and the filing module.

use ncrs_hermes_chat::{ChipReply, Entry, Part};

use crate::app::App;

pub fn chat(app: &App) -> &ncrs_hermes_chat::State {
    app.hermes_chat_for_test()
}

pub fn busy(app: &App) -> bool {
    let chat = chat(app);
    chat.pending_work().is_some()
        || chat.pending_turn().is_some()
        || chat.pending_resolve().is_some()
        || chat.pending_naming().is_some()
        || chat.pending_rule_ask().is_some()
        || chat.waits_for_hermes()
}

pub fn entry_text(entry: &Entry) -> String {
    match entry {
        Entry::User(text) | Entry::Working(text) => text.clone(),
        Entry::UserFolder(mention) => format!("{} ({})", mention.name, mention.path),
        Entry::Hermes { parts, .. } => parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => text.clone(),
                Part::Areas(card) => card
                    .rows
                    .iter()
                    .map(|row| format!("[{} | {} | {}]", row.name, row.description, row.example))
                    .collect::<Vec<_>>()
                    .join(" "),
                Part::File(card) => card.segments.join("/"),
                Part::Levels(card) => card
                    .rows
                    .iter()
                    .map(|row| format!("{}. {}", row.number, row.text))
                    .collect::<Vec<_>>()
                    .join(" "),
                Part::Folder(mention) => format!("{} ({})", mention.name, mention.path),
                Part::Trial(card) => format!(
                    "{} {}",
                    card.header,
                    card.rows
                        .iter()
                        .map(|row| row.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Whether the newest areas card writes the order of every area in words:
/// no slashes or bars between the levels.
pub fn areas_in_plain_words(app: &App) -> bool {
    chat(app)
        .thread()
        .iter()
        .rev()
        .find_map(|entry| match entry {
            Entry::Hermes { parts, .. } => parts.iter().find_map(|part| match part {
                Part::Areas(card) => Some(card),
                _ => None,
            }),
            _ => None,
        })
        .is_some_and(|card| {
            card.rows
                .iter()
                .all(|row| !row.description.contains(" / ") && !row.description.contains(" | "))
        })
}

pub fn whole_thread(app: &App) -> String {
    chat(app)
        .thread()
        .iter()
        .map(entry_text)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn last_text(app: &App) -> String {
    chat(app)
        .thread()
        .last()
        .map(entry_text)
        .unwrap_or_default()
}

pub fn last_working_has_digit(app: &App) -> bool {
    matches!(chat(app).thread().last(), Some(Entry::Working(text)) if text.chars().any(|c| c.is_ascii_digit()))
}

pub fn last_hermes_has_chips(app: &App) -> bool {
    matches!(chat(app).thread().last(), Some(Entry::Hermes { chips, .. }) if !chips.is_empty())
}

pub fn has_chip(app: &App, wanted: impl Fn(&ChipReply) -> bool) -> bool {
    chat(app).chips().iter().any(|chip| wanted(&chip.reply))
}

pub fn chip_labels(app: &App) -> String {
    chat(app)
        .chips()
        .iter()
        .map(|chip| chip.label.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

pub fn is_ready(app: &App) -> bool {
    app.filing_module_for_test()
        .is_some_and(|module| matches!(module.phase(), crate::filing::Phase::Ready))
}

pub fn documents_indexed(app: &App) -> Option<u64> {
    let figures = app
        .filing_module_for_test()?
        .index_figures_reader()?
        .read()
        .ok()?;
    Some(figures.statistics.document_count)
}
