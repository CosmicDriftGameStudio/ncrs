//! The Platz answer: the card and its figures, built from a scan. What the
//! user reads comes from `strings.json`; the model only fills the hint of
//! the folders the user decides about.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use iced::{Color, Element};
use ncrs_ai::FolderFacts;
use ncrs_i18n::{Language, Msg};
use ncrs_space::decide::DecideKind;
use ncrs_space::{
    cargo_project_name, catalog_msgs, CatalogId, Entry, EntryKind, Group, Scan, UseCheck,
};
use ncrs_ui_kit::format;
use ncrs_ui_kit::hermes_kit::{space_card, SpaceCardRow, SpaceCardView, SpaceLegendItem};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::stacked_bar::BarSegment;
use ncrs_ui_kit::theme;

use crate::ctx::Ctx;
use crate::message::Message;
use crate::texts::{fill, folder_name, shown_folder, text};

/// The rows of the table; the rest goes into one line below it.
const TABLE_ROWS: usize = 7;
const ALSO_ROWS: usize = 3;
const SECONDS_PER_MINUTE: u64 = 60;
const SHOWN_PARTS: usize = 3;

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
    pub bytes: u64,
    /// The size by group, for the small bar of the row.
    pub parts: Vec<(SpaceBand, u64)>,
    /// One chip per group the row has.
    pub chips: Vec<(SpaceBand, String)>,
    /// The labels of `chips`, joined.
    pub group: String,
    /// The reason the code gives, or the sentence the model wrote for it.
    pub hint: String,
    /// The model is asked for this row's hint.
    pub described: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceLegend {
    pub band: SpaceBand,
    pub label: String,
    pub size: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceCard {
    pub title: String,
    pub drive: String,
    pub bar: Vec<(SpaceBand, u64)>,
    pub legend: Vec<SpaceLegend>,
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

/// The chip of a group on a row.
fn group_label(lang: Language, band: SpaceBand) -> String {
    text(
        lang,
        match band {
            SpaceBand::Safe => Msg::SpaceGroupSafeResult,
            SpaceBand::Worktrees => Msg::SpaceLegendWorktrees,
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

/// The group in lower case, as it stands inside a sentence.
fn group_word(lang: Language, band: SpaceBand) -> String {
    text(lang, legend_label(band))
}

fn is_build(id: CatalogId) -> bool {
    matches!(
        id,
        CatalogId::CargoTarget
            | CatalogId::CargoTargetHome
            | CatalogId::DerivedData
            | CatalogId::Gradle
    )
}

/// The name of a catalog folder and why it can go.
fn catalog_texts(ctx: &Ctx, id: CatalogId, entry: &Entry) -> (String, String) {
    let lang = ctx.lang;
    let pair = |name: Msg, why: Msg| (text(lang, name), text(lang, why));
    match id {
        CatalogId::AppCache(name) => (
            fill(lang, Msg::SpaceCatalogXdgCache, &[("name", name)]),
            text(lang, Msg::SpaceCatalogXdgCacheWhy),
        ),
        CatalogId::OwnRegenerable => (
            folder_name(ctx, &entry.path),
            text(lang, Msg::SpaceGroupSafeResultHint),
        ),
        CatalogId::CargoTarget | CatalogId::CargoTargetHome => {
            let (kind, why) = catalog_msgs(id);
            let name = match cargo_project_name(&entry.path) {
                Some(project) => fill(
                    lang,
                    Msg::HermesChatSpaceRowRustBuild,
                    &[("project", &project)],
                ),
                None => text(lang, kind),
            };
            (name, text(lang, why))
        }
        other => {
            let (name, why) = catalog_msgs(other);
            pair(name, why)
        }
    }
}

fn decide_name_and_check(ctx: &Ctx, entry: &Entry, kind: &DecideKind) -> (String, String) {
    let lang = ctx.lang;
    let pair = |name: Msg, check: Msg| (text(lang, name), text(lang, check));
    match kind {
        DecideKind::SessionLogs => pair(
            Msg::SpaceDecideSessionLogs,
            Msg::SpaceDecideSessionLogsCheck,
        ),
        DecideKind::Backups => pair(Msg::SpaceDecideBackups, Msg::SpaceDecideBackupsCheck),
        DecideKind::ModelCache => pair(Msg::SpaceDecideModelCache, Msg::SpaceDecideModelCacheCheck),
        DecideKind::Simulators => pair(Msg::SpaceDecideSimulators, Msg::SpaceDecideSimulatorsCheck),
        _ => (
            folder_name(ctx, &entry.path),
            text(lang, Msg::SpaceDecideOtherCheck),
        ),
    }
}

fn holder_name(entry: &Entry) -> Option<&str> {
    entry.held_by.first().map(|holder| holder.name.as_str())
}

/// For a safe folder: when it was last used and that the check found nothing using it.
fn checked_hint(ctx: &Ctx, entry: &Entry, id: CatalogId, why: String) -> String {
    if entry.facts.use_check != UseCheck::Free {
        return why;
    }
    let Some(used) = entry.facts.last_used.or(entry.modified) else {
        return why;
    };
    let date = DateTime::<Local>::from(used).format("%d.%m.").to_string();
    let msg = if is_build(id) {
        Msg::HermesChatSpaceHintLastFreeBuild
    } else {
        Msg::HermesChatSpaceHintLastFree
    };
    fill(ctx.lang, msg, &[("date", &date)])
}

/// The name of an entry and the reason the code gives for its group.
fn name_and_reason(ctx: &Ctx, entry: &Entry) -> (String, String) {
    let lang = ctx.lang;
    match (&entry.kind, entry.group) {
        (EntryKind::Catalog(id), Group::InUse) => {
            let (name, _) = catalog_texts(ctx, *id, entry);
            (name, in_use_hint(ctx, entry))
        }
        (_, Group::InUse) => (folder_name(ctx, &entry.path), in_use_hint(ctx, entry)),
        (EntryKind::Catalog(id), _) => {
            let (name, why) = catalog_texts(ctx, *id, entry);
            let hint = checked_hint(ctx, entry, *id, why);
            (name, hint)
        }
        (EntryKind::Worktree(worktree), _) => (
            if worktree.branch.is_empty() {
                folder_name(ctx, &entry.path)
            } else {
                worktree.branch.clone()
            },
            text(lang, Msg::SpaceWorktreeCheck),
        ),
        (EntryKind::Decide(kind), _) => decide_name_and_check(ctx, entry, kind),
    }
}

fn in_use_hint(ctx: &Ctx, entry: &Entry) -> String {
    match holder_name(entry) {
        Some(holder) => fill(
            ctx.lang,
            Msg::HermesChatSpaceHintInUse,
            &[("holder", holder)],
        ),
        None => text(ctx.lang, Msg::SpaceGroupInUseHint),
    }
}

/// What a kind of entry is called when several of them are counted together.
fn kind_name(ctx: &Ctx, entry: &Entry) -> String {
    match &entry.kind {
        EntryKind::Catalog(id @ (CatalogId::CargoTarget | CatalogId::CargoTargetHome)) => {
            text(ctx.lang, catalog_msgs(*id).0)
        }
        _ => name_and_reason(ctx, entry).0,
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

/// What entries lie together in one row of the table.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Place {
    CacheRoot(PathBuf),
    Worktrees(PathBuf),
    Downloads,
    Simulators,
    Single(usize),
}

fn place_of(ctx: &Ctx, index: usize, entry: &Entry) -> Place {
    let parent = entry.path.parent().map(Path::to_path_buf);
    let cache_roots = [ctx.home.join("Library/Caches"), ctx.home.join(".cache")];
    match &entry.kind {
        EntryKind::Worktree(_) => return Place::Worktrees(parent.unwrap_or_default()),
        EntryKind::Decide(DecideKind::Simulators)
        | EntryKind::Catalog(CatalogId::UnavailableSimulators) => return Place::Simulators,
        _ => {}
    }
    if let Some(root) = parent.filter(|parent| cache_roots.contains(parent)) {
        return Place::CacheRoot(root);
    }
    if entry.group == Group::Decide && entry.path.starts_with(ctx.home.join("Downloads")) {
        return Place::Downloads;
    }
    Place::Single(index)
}

struct Draft {
    row: SpaceRow,
    kind: String,
    facts: Option<FolderFacts>,
}

fn parts_of(entries: &[&Entry]) -> Vec<(SpaceBand, u64)> {
    [Group::Safe, Group::Worktrees, Group::Decide, Group::InUse]
        .into_iter()
        .filter_map(|group| {
            let bytes = entries
                .iter()
                .filter(|entry| entry.group == group)
                .map(|entry| entry.size.bytes)
                .fold(0_u64, u64::saturating_add);
            (bytes > 0).then_some((group_band(group), bytes))
        })
        .collect()
}

fn draft(
    ctx: &Ctx,
    name: String,
    path: String,
    parts: Vec<(SpaceBand, u64)>,
    hint: String,
) -> Draft {
    let lang = ctx.lang;
    let bytes = parts
        .iter()
        .map(|(_, bytes)| *bytes)
        .fold(0_u64, u64::saturating_add);
    let chips: Vec<(SpaceBand, String)> = parts
        .iter()
        .map(|(band, _)| *band)
        .filter(|band| *band != SpaceBand::Rest)
        .map(|band| (band, group_label(lang, band)))
        .collect();
    let group = chips
        .iter()
        .map(|(_, label)| label.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Draft {
        kind: name.clone(),
        facts: None,
        row: SpaceRow {
            name,
            path,
            size: format::size_shown(bytes, lang),
            bytes,
            parts,
            chips,
            group,
            hint,
            described: false,
        },
    }
}

fn single_draft(ctx: &Ctx, entry: &Entry) -> Draft {
    let (name, hint) = name_and_reason(ctx, entry);
    let described = entry.group == Group::Decide
        && !matches!(&entry.kind, EntryKind::Decide(DecideKind::Simulators));
    let mut made = draft(
        ctx,
        name,
        shown_folder(ctx, &entry.path),
        parts_of(&[entry]),
        hint,
    );
    made.kind = kind_name(ctx, entry);
    made.row.described = described;
    if described {
        made.facts = Some(FolderFacts {
            path: shown_folder(ctx, &entry.path),
            size_bytes: entry.size.bytes,
            modified: entry.modified.map(date_of),
        });
    }
    made
}

/// The parts of a row that lists what lies in it: "sccache 10 GB, Yarn 4,5 GB sicher · Google 5 GB: Chrome läuft".
fn parts_hint(ctx: &Ctx, entries: &[&Entry]) -> String {
    let lang = ctx.lang;
    let shown = |bytes: u64| format::size_shown(bytes, lang);
    let mut pieces = Vec::new();
    for group in [Group::Safe, Group::Worktrees, Group::Decide, Group::InUse] {
        let mut members: Vec<&&Entry> = entries
            .iter()
            .filter(|entry| entry.group == group)
            .collect();
        members.sort_by(|left, right| right.size.bytes.cmp(&left.size.bytes));
        let listed: Vec<String> = members
            .iter()
            .take(SHOWN_PARTS)
            .map(|entry| {
                let name = name_and_reason(ctx, entry).0;
                match (group, holder_name(entry)) {
                    (Group::InUse, Some(holder)) => fill(
                        lang,
                        Msg::HermesChatSpaceHintInUsePart,
                        &[
                            ("name", &name),
                            ("size", &shown(entry.size.bytes)),
                            ("holder", holder),
                        ],
                    ),
                    _ => format!("{name} {}", shown(entry.size.bytes)),
                }
            })
            .collect();
        if listed.is_empty() {
            continue;
        }
        if group == Group::InUse {
            pieces.push(listed.join(", "));
        } else {
            pieces.push(fill(
                lang,
                Msg::HermesChatSpaceHintPart,
                &[
                    ("items", &listed.join(", ")),
                    ("group", &group_word(lang, group_band(group))),
                ],
            ));
        }
    }
    pieces.join(" \u{b7} ")
}

fn worktree_draft(ctx: &Ctx, scan: &Scan, folder: &Path, entries: &[&Entry]) -> Draft {
    let lang = ctx.lang;
    let shown = |bytes: u64| format::size_shown(bytes, lang);
    let (count, total, merged_count, merged, kept_count) = scan
        .worktree_roots
        .iter()
        .find(|root| root.folder == folder)
        .map_or_else(
            || {
                let total = entries.iter().map(|entry| entry.size.bytes).sum();
                (entries.len(), total, entries.len(), total, 0)
            },
            |root| {
                (
                    root.total.count,
                    root.total.bytes,
                    root.merged.count,
                    root.merged.bytes,
                    root.kept.count,
                )
            },
        );
    let mut parts = vec![(SpaceBand::Worktrees, merged)];
    if total > merged {
        parts.push((SpaceBand::Rest, total - merged));
    }
    let path = format!(
        "{} \u{b7} {}",
        shown_folder(ctx, folder),
        fill(
            lang,
            Msg::HermesChatSpaceWorktreeCount,
            &[("count", &count.to_string())]
        )
    );
    let hint = fill(
        lang,
        Msg::HermesChatSpaceWorktreesHint,
        &[
            ("merged", &merged_count.to_string()),
            ("size", &shown(merged)),
            ("kept", &kept_count.to_string()),
        ],
    );
    let mut made = draft(
        ctx,
        text(lang, Msg::SpaceLegendWorktrees),
        path,
        parts,
        hint,
    );
    made.row.chips = vec![(
        SpaceBand::Worktrees,
        group_label(lang, SpaceBand::Worktrees),
    )];
    made.row.group = group_label(lang, SpaceBand::Worktrees);
    made
}

fn simulators_draft(ctx: &Ctx, entries: &[&Entry]) -> Draft {
    let lang = ctx.lang;
    let bytes_of = |group: Group| {
        entries
            .iter()
            .filter(|entry| entry.group == group)
            .map(|entry| entry.size.bytes)
            .fold(0_u64, u64::saturating_add)
    };
    let hint = fill(
        lang,
        Msg::HermesChatSpaceSimulatorsHint,
        &[
            ("gone", &format::size_shown(bytes_of(Group::Safe), lang)),
            ("usable", &format::size_shown(bytes_of(Group::Decide), lang)),
        ],
    );
    let path = entries
        .first()
        .and_then(|entry| entry.path.parent())
        .map(|parent| shown_folder(ctx, parent))
        .unwrap_or_default();
    draft(
        ctx,
        text(lang, Msg::HermesChatSpaceRowSimulators),
        path,
        parts_of(entries),
        hint,
    )
}

fn drafts(ctx: &Ctx, scan: &Scan) -> Vec<Draft> {
    let mut order: Vec<Place> = Vec::new();
    let mut members: HashMap<Place, Vec<&Entry>> = HashMap::new();
    for (index, entry) in scan.entries.iter().enumerate() {
        let place = place_of(ctx, index, entry);
        if !members.contains_key(&place) {
            order.push(place.clone());
        }
        members.entry(place).or_default().push(entry);
    }
    let mut made: Vec<Draft> = order
        .into_iter()
        .filter_map(|place| {
            let entries = members.remove(&place)?;
            Some(match place {
                Place::Single(_) => single_draft(ctx, entries.first()?),
                Place::CacheRoot(root) => draft(
                    ctx,
                    text(ctx.lang, Msg::HermesChatSpaceRowCaches),
                    shown_folder(ctx, &root),
                    parts_of(&entries),
                    parts_hint(ctx, &entries),
                ),
                Place::Downloads => draft(
                    ctx,
                    text(ctx.lang, Msg::SpaceDecideDownloads),
                    shown_folder(ctx, &ctx.home.join("Downloads")),
                    parts_of(&entries),
                    text(ctx.lang, Msg::SpaceDecideDownloadsCheck),
                ),
                Place::Simulators => simulators_draft(ctx, &entries),
                Place::Worktrees(folder) => worktree_draft(ctx, scan, &folder, &entries),
            })
        })
        .collect();
    made.sort_by(|left, right| right.row.bytes.cmp(&left.row.bytes));
    made
}

/// The rows that did not fit, counted by kind: "Rust-Build ×3 11 GB (sicher)".
fn also_line(ctx: &Ctx, rest: &[Draft]) -> Option<String> {
    let lang = ctx.lang;
    let mut kinds: Vec<(String, SpaceBand, usize, u64)> = Vec::new();
    for draft in rest {
        let band = draft
            .row
            .parts
            .iter()
            .max_by_key(|(_, bytes)| *bytes)
            .map_or(SpaceBand::Rest, |(band, _)| *band);
        match kinds
            .iter_mut()
            .find(|(kind, kind_band, _, _)| *kind == draft.kind && *kind_band == band)
        {
            Some(found) => {
                found.2 += 1;
                found.3 = found.3.saturating_add(draft.row.bytes);
            }
            None => kinds.push((draft.kind.clone(), band, 1, draft.row.bytes)),
        }
    }
    let pieces: Vec<String> = kinds
        .into_iter()
        .take(ALSO_ROWS)
        .map(|(kind, band, count, bytes)| {
            let size = format::size_shown(bytes, lang);
            let group = group_word(lang, band);
            if count == 1 {
                fill(
                    lang,
                    Msg::HermesChatSpaceAlsoOne,
                    &[("name", &kind), ("size", &size), ("group", &group)],
                )
            } else {
                fill(
                    lang,
                    Msg::HermesChatSpaceAlsoMany,
                    &[
                        ("name", &kind),
                        ("count", &count.to_string()),
                        ("size", &size),
                        ("group", &group),
                    ],
                )
            }
        })
        .collect();
    (!pieces.is_empty()).then(|| {
        fill(
            lang,
            Msg::HermesChatSpaceCardAlso,
            &[("rest", &pieces.join(" \u{b7} "))],
        )
    })
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
        .map(|(band, bytes)| SpaceLegend {
            band: *band,
            label: text(lang, legend_label(*band)),
            size: shown(*bytes),
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

    let mut all = drafts(ctx, scan);
    let rest = all.split_off(TABLE_ROWS.min(all.len()));
    let also = also_line(ctx, &rest);
    let facts: Vec<FolderFacts> = all
        .iter_mut()
        .filter_map(|made| made.facts.take())
        .collect();
    let rows = all.into_iter().map(|made| made.row).collect();

    let drive_name = if scan.volume_name.is_empty() {
        &ctx.drive_name
    } else {
        &scan.volume_name
    };
    let card = SpaceCard {
        title: text(lang, Msg::HermesChatSpaceCardTitle),
        drive: fill(
            lang,
            Msg::HermesChatSpaceCardDrive,
            &[("drive", drive_name), ("total", &shown(total))],
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

pub fn view<'a>(
    card: &SpaceCard,
    actions: Option<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
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
            .map(|item| SpaceLegendItem {
                label: item.label.clone(),
                size: item.size.clone(),
                color: band_color(item.band, palette),
            })
            .collect(),
        after: card.after.clone(),
        columns: card.columns.clone(),
        rows: card
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| SpaceCardRow {
                name: row.name.clone(),
                path: row.path.clone(),
                size: row.size.clone(),
                parts: row
                    .parts
                    .iter()
                    .map(|(band, bytes)| BarSegment {
                        portion: *bytes,
                        color: band_color(*band, palette),
                    })
                    .collect(),
                groups: row
                    .chips
                    .iter()
                    .map(|(band, label)| (label.clone(), band_color(*band, palette)))
                    .collect(),
                hint: row.hint.clone(),
                selected: index == 0,
            })
            .collect(),
        also: card.also.clone(),
        footer: card.footer.clone(),
    };
    space_card(&shown, actions, palette)
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
