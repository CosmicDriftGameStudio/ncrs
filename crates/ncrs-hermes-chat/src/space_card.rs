//! The Platz answer: the card and its figures, built from a scan. What the
//! user reads comes from `strings.json`; the model only fills the hint of
//! the folders the user decides about.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use iced::{Color, Element};
use ncrs_ai::FolderFacts;
use ncrs_i18n::{Language, Msg};
use ncrs_space::{CatalogId, Entry, EntryKind, Group, Scan};
use ncrs_ui_kit::format;
use ncrs_ui_kit::hermes_kit::{space_card, SpaceCardRow, SpaceCardView};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::stacked_bar::BarSegment;
use ncrs_ui_kit::theme;

use crate::ctx::Ctx;
use crate::message::Message;
use crate::texts::{fill, folder_name, shown_folder, text};

/// The rows of the table; the rest goes into one line below it.
const TABLE_ROWS: usize = 6;
const ALSO_ROWS: usize = 3;
const SECONDS_PER_MINUTE: u64 = 60;

/// A part of the bar, in the order of the legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceBand {
    Safe,
    Worktrees,
    Decide,
    InUse,
    Rest,
    Free,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRow {
    pub name: String,
    pub path: String,
    pub size: String,
    pub band: SpaceBand,
    pub group: String,
    /// The reason the code gives, or the sentence the model wrote for it.
    pub hint: String,
    /// The model is asked for this row's hint.
    pub described: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceCard {
    pub title: String,
    pub drive: String,
    pub bar: Vec<(SpaceBand, u64)>,
    pub legend: Vec<(SpaceBand, String)>,
    pub after: Option<String>,
    pub columns: [String; 4],
    pub rows: Vec<SpaceRow>,
    pub also: Option<String>,
    pub footer: String,
}

/// The card with the figures the chips and sentences around it need.
pub struct Built {
    pub card: SpaceCard,
    pub intro: String,
    pub safe_bytes: u64,
    pub worktree_bytes: u64,
    /// What the model is asked about, in the order of the rows it fills.
    pub facts: Vec<FolderFacts>,
}

pub fn percent(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    part.saturating_mul(100).saturating_add(whole / 2) / whole
}

fn took_text(took: Duration) -> String {
    let seconds = took.as_secs();
    if seconds >= SECONDS_PER_MINUTE {
        format!(
            "{} min {} s",
            seconds / SECONDS_PER_MINUTE,
            seconds % SECONDS_PER_MINUTE
        )
    } else {
        format!("{seconds} s")
    }
}

fn group_band(group: Group) -> SpaceBand {
    match group {
        Group::Safe => SpaceBand::Safe,
        Group::Worktrees => SpaceBand::Worktrees,
        Group::Decide => SpaceBand::Decide,
        Group::InUse => SpaceBand::InUse,
    }
}

fn group_label(lang: Language, band: SpaceBand) -> String {
    text(
        lang,
        match band {
            SpaceBand::Safe => Msg::SpaceGroupSafeResult,
            SpaceBand::Worktrees => Msg::SpaceGroupWorktrees,
            SpaceBand::Decide => Msg::SpaceGroupDecide,
            SpaceBand::InUse => Msg::SpaceGroupInUse,
            SpaceBand::Rest => Msg::SpaceLegendRest,
            SpaceBand::Free => Msg::SpaceLegendFree,
        },
    )
}

fn legend_label(band: SpaceBand) -> Msg {
    match band {
        SpaceBand::Safe => Msg::SpaceLegendSafe,
        SpaceBand::Worktrees => Msg::SpaceLegendWorktrees,
        SpaceBand::Decide => Msg::SpaceLegendDecide,
        SpaceBand::InUse => Msg::SpaceLegendInUse,
        SpaceBand::Rest => Msg::SpaceLegendRest,
        SpaceBand::Free => Msg::SpaceLegendFree,
    }
}

/// The name of a catalog folder and why it can go.
fn catalog_texts(ctx: &Ctx, id: CatalogId, entry: &Entry) -> (String, String) {
    let lang = ctx.lang;
    let pair = |name: Msg, why: Msg| (text(lang, name), text(lang, why));
    match id {
        CatalogId::CargoTarget => pair(Msg::SpaceCatalogCargoTarget, Msg::SpaceCatalogCargoTargetWhy),
        CatalogId::Sccache => pair(Msg::SpaceCatalogSccache, Msg::SpaceCatalogSccacheWhy),
        CatalogId::CargoRegistryCache => pair(
            Msg::SpaceCatalogCargoRegistryCache,
            Msg::SpaceCatalogCargoRegistryCacheWhy,
        ),
        CatalogId::Yarn => pair(Msg::SpaceCatalogYarn, Msg::SpaceCatalogYarnWhy),
        CatalogId::Npm => pair(Msg::SpaceCatalogNpm, Msg::SpaceCatalogNpmWhy),
        CatalogId::Bun => pair(Msg::SpaceCatalogBun, Msg::SpaceCatalogBunWhy),
        CatalogId::Pnpm => pair(Msg::SpaceCatalogPnpm, Msg::SpaceCatalogPnpmWhy),
        CatalogId::Pip => pair(Msg::SpaceCatalogPip, Msg::SpaceCatalogPipWhy),
        CatalogId::Homebrew => pair(Msg::SpaceCatalogHomebrew, Msg::SpaceCatalogHomebrewWhy),
        CatalogId::DerivedData => pair(Msg::SpaceCatalogDerivedData, Msg::SpaceCatalogDerivedDataWhy),
        CatalogId::UnavailableSimulators => pair(
            Msg::SpaceCatalogUnavailableSimulators,
            Msg::SpaceCatalogUnavailableSimulatorsWhy,
        ),
        CatalogId::Gradle => pair(Msg::SpaceCatalogGradle, Msg::SpaceCatalogGradleWhy),
        CatalogId::AppCache(name) => (
            fill(lang, Msg::SpaceCatalogXdgCache, &[("name", name)]),
            text(lang, Msg::SpaceCatalogXdgCacheWhy),
        ),
        CatalogId::WinTempOld => pair(Msg::SpaceCatalogWinTempOld, Msg::SpaceCatalogWinTempOldWhy),
        CatalogId::OwnRegenerable => (
            folder_name(ctx, &entry.path),
            text(lang, Msg::SpaceGroupSafeResultHint),
        ),
    }
}

/// The name of an entry and the reason the code gives for its group.
fn name_and_reason(ctx: &Ctx, entry: &Entry) -> (String, String) {
    let lang = ctx.lang;
    match (&entry.kind, entry.group) {
        (_, Group::InUse) => (
            folder_name(ctx, &entry.path),
            text(lang, Msg::SpaceGroupInUseHint),
        ),
        (EntryKind::Catalog(id), _) => catalog_texts(ctx, *id, entry),
        (EntryKind::Worktree(worktree), _) => (
            if worktree.branch.is_empty() {
                folder_name(ctx, &entry.path)
            } else {
                worktree.branch.clone()
            },
            text(lang, Msg::SpaceWorktreeCheck),
        ),
        (EntryKind::Decide(_), _) => (
            folder_name(ctx, &entry.path),
            text(lang, Msg::SpaceDecideOtherCheck),
        ),
    }
}

fn date_of(time: SystemTime) -> String {
    DateTime::<Local>::from(time).format("%Y-%m-%d").to_string()
}

fn sum(scan: &Scan, group: Group) -> u64 {
    scan.entries
        .iter()
        .filter(|entry| entry.group == group)
        .map(|entry| entry.size.bytes)
        .fold(0_u64, u64::saturating_add)
}

pub fn build(scan: &Scan, ctx: &Ctx) -> Built {
    let lang = ctx.lang;
    let total = scan.volume.total;
    let free = scan.volume.available;
    let used = total.saturating_sub(free);
    let safe_bytes = sum(scan, Group::Safe);
    let worktree_bytes = sum(scan, Group::Worktrees);
    let decide_bytes = sum(scan, Group::Decide);
    let in_use_bytes = sum(scan, Group::InUse);
    let listed = safe_bytes
        .saturating_add(worktree_bytes)
        .saturating_add(decide_bytes)
        .saturating_add(in_use_bytes);
    let rest_bytes = used.saturating_sub(listed);
    let shown = |bytes: u64| format::size_shown(bytes, lang);

    let bar = vec![
        (SpaceBand::Safe, safe_bytes),
        (SpaceBand::Worktrees, worktree_bytes),
        (SpaceBand::Decide, decide_bytes),
        (SpaceBand::InUse, in_use_bytes),
        (SpaceBand::Rest, rest_bytes),
        (SpaceBand::Free, free),
    ];
    let legend = bar
        .iter()
        .filter(|(_, bytes)| *bytes > 0)
        .map(|(band, bytes)| {
            let label = text(lang, legend_label(*band));
            (
                *band,
                fill(
                    lang,
                    Msg::SpaceLegendEntry,
                    &[("label", &label), ("size", &shown(*bytes))],
                ),
            )
        })
        .collect();

    let after = (safe_bytes > 0).then(|| {
        let free_after = free.saturating_add(safe_bytes);
        fill(
            lang,
            Msg::HermesChatSpaceCardAfter,
            &[
                ("free", &shown(free_after)),
                (
                    "percent",
                    &percent(total.saturating_sub(free_after), total).to_string(),
                ),
            ],
        )
    });

    let mut ordered: Vec<&Entry> = scan.entries.iter().collect();
    ordered.sort_by(|left, right| right.size.bytes.cmp(&left.size.bytes));
    let mut facts = Vec::new();
    let rows = ordered
        .iter()
        .take(TABLE_ROWS)
        .map(|entry| {
            let band = group_band(entry.group);
            let (name, reason) = name_and_reason(ctx, entry);
            let described = entry.group == Group::Decide;
            if described {
                facts.push(FolderFacts {
                    path: shown_folder(ctx, &entry.path),
                    size_bytes: entry.size.bytes,
                    modified: entry.modified.map(date_of),
                });
            }
            SpaceRow {
                name,
                path: shown_folder(ctx, &entry.path),
                size: shown(entry.size.bytes),
                band,
                group: group_label(lang, band),
                hint: reason,
                described,
            }
        })
        .collect();
    let also_parts: Vec<String> = ordered
        .iter()
        .skip(TABLE_ROWS)
        .take(ALSO_ROWS)
        .map(|entry| {
            let (name, _) = name_and_reason(ctx, entry);
            format!(
                "{name} {} ({})",
                shown(entry.size.bytes),
                group_label(lang, group_band(entry.group))
            )
        })
        .collect();
    let also = (!also_parts.is_empty()).then(|| {
        fill(
            lang,
            Msg::HermesChatSpaceCardAlso,
            &[("rest", &also_parts.join(" \u{b7} "))],
        )
    });

    let card = SpaceCard {
        title: text(lang, Msg::HermesChatSpaceCardTitle),
        drive: fill(
            lang,
            Msg::HermesChatSpaceCardDrive,
            &[("drive", &ctx.drive_name), ("total", &shown(total))],
        ),
        bar,
        legend,
        after,
        columns: [
            text(lang, Msg::HermesChatSpaceCardColumnWhat),
            text(lang, Msg::HermesChatSpaceCardColumnSize),
            text(lang, Msg::HermesChatSpaceCardColumnGroup),
            text(lang, Msg::HermesChatSpaceCardColumnHint),
        ],
        rows,
        also,
        footer: text(lang, Msg::HermesChatSpaceCardFooter),
    };
    let intro = fill(
        lang,
        Msg::HermesChatSpaceIntro,
        &[
            ("used", &shown(used)),
            ("total", &shown(total)),
            ("free", &shown(free)),
            ("percent", &percent(used, total).to_string()),
            ("took", &took_text(scan.took)),
        ],
    );
    Built {
        card,
        intro,
        safe_bytes,
        worktree_bytes,
        facts,
    }
}

fn band_color(band: SpaceBand, palette: &Palette) -> Color {
    match band {
        SpaceBand::Safe => theme::space::safe(palette),
        SpaceBand::Worktrees => theme::space::worktrees(),
        SpaceBand::Decide => theme::space::decide(),
        SpaceBand::InUse => theme::space::in_use(palette),
        SpaceBand::Rest => theme::space::rest(palette),
        SpaceBand::Free => theme::space::free(palette),
    }
}

pub fn view<'a>(card: &SpaceCard, palette: &Palette) -> Element<'a, Message> {
    let shown = SpaceCardView {
        title: card.title.clone(),
        drive: card.drive.clone(),
        bar: card
            .bar
            .iter()
            .map(|(band, bytes)| BarSegment {
                portion: *bytes,
                color: band_color(*band, palette),
            })
            .collect(),
        legend: card
            .legend
            .iter()
            .map(|(band, label)| (label.clone(), band_color(*band, palette)))
            .collect(),
        after: card.after.clone(),
        columns: card.columns.clone(),
        rows: card
            .rows
            .iter()
            .map(|row| SpaceCardRow {
                name: row.name.clone(),
                path: row.path.clone(),
                size: row.size.clone(),
                group: row.group.clone(),
                group_color: band_color(row.band, palette),
                hint: row.hint.clone(),
            })
            .collect(),
        also: card.also.clone(),
        footer: card.footer.clone(),
    };
    space_card(&shown, palette)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_is_rounded_to_a_whole_percent() {
        assert_eq!(percent(848, 926), 92);
        assert_eq!(percent(0, 0), 0);
    }

    #[test]
    fn a_long_scan_is_told_in_minutes() {
        assert_eq!(took_text(Duration::from_secs(72)), "1 min 12 s");
        assert_eq!(took_text(Duration::from_secs(9)), "9 s");
    }
}
