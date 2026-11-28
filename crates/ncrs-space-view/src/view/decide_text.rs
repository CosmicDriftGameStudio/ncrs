//! The words of the decide pane: group titles and hints, the reason of a row, the kind of a file.

use std::path::Path;

use ncrs_i18n::{fill_template, Msg};
use ncrs_space::{InstalledApp, VersionRelation};
use ncrs_ui_kit::format::{shown_path, time};

use crate::ctx::ViewCtx;
use crate::decide::{DecideGroupKind, DecideReason, DecideState};
use crate::shortcuts::Shortcut;
use crate::texts::fill_plural;

pub fn group_msg(kind: DecideGroupKind) -> Msg {
    match kind {
        DecideGroupKind::Installed => Msg::SpaceDecideGroupInstalled,
        DecideGroupKind::Unpacked => Msg::SpaceDecideGroupUnpacked,
        DecideGroupKind::Duplicates => Msg::SpaceDecideGroupDuplicates,
        DecideGroupKind::UntouchedYear => Msg::SpaceDecideGroupUntouchedYear,
        DecideGroupKind::Rest => Msg::SpaceDecideGroupRest,
    }
}

pub fn hint_msg(kind: DecideGroupKind) -> Msg {
    match kind {
        DecideGroupKind::Installed => Msg::SpaceDecideHintInstalled,
        DecideGroupKind::Unpacked => Msg::SpaceDecideHintUnpacked,
        DecideGroupKind::Duplicates => Msg::SpaceDecideHintDuplicates,
        DecideGroupKind::UntouchedYear => Msg::SpaceDecideHintUntouched,
        DecideGroupKind::Rest => Msg::SpaceDecideHintRest,
    }
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The name of an installed app: the folder name of the app without `.app`.
fn app_name(app: &InstalledApp) -> String {
    app.path.file_stem().map_or_else(
        || file_name(&app.path),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

/// The ISO date of a time, without the clock.
pub fn day(at: Option<std::time::SystemTime>) -> String {
    time(at).chars().take(10).collect()
}

/// The reason of a row with the concrete fact.
pub fn reason_text(reason: &DecideReason, ctx: ViewCtx<'_>) -> String {
    match reason {
        DecideReason::Installed(app) => match app.relation {
            VersionRelation::InstalledNewer if app.version.is_some() => fill_template(
                ctx.lang.text(Msg::SpaceDecideRowInstalledNewer),
                &[("{version}", app.version.as_deref().unwrap_or_default())],
            ),
            VersionRelation::Same => fill_template(
                ctx.lang.text(Msg::SpaceDecideRowInstalledSame),
                &[("{app}", &app_name(app))],
            ),
            _ => fill_template(
                ctx.lang.text(Msg::SpaceDecideRowInstalled),
                &[("{app}", &app_name(app))],
            ),
        },
        DecideReason::Unpacked(folder) => fill_template(
            ctx.lang.text(Msg::SpaceDecideRowUnpacked),
            &[("{folder}", &shown_path(folder, ctx.home))],
        ),
        DecideReason::Duplicate(copy) => fill_template(
            ctx.lang.text(Msg::SpaceDecideRowDuplicate),
            &[("{path}", &shown_path(copy, ctx.home))],
        ),
        DecideReason::Untouched => ctx
            .lang
            .text(Msg::SpaceDecideGroupReasonUntouched)
            .to_owned(),
        DecideReason::Rest => String::new(),
    }
}

/// The lines under "why Hermes suggests it", each one a fact the scan found.
pub fn why_lines(reason: &DecideReason, file: &Path, ctx: ViewCtx<'_>) -> Vec<String> {
    let lang = ctx.lang;
    match reason {
        DecideReason::Installed(app) => {
            let named = [
                ("{app}", app_name(app)),
                ("{path}", shown_path(&app.path, ctx.home)),
                ("{version}", app.version.clone().unwrap_or_default()),
            ];
            let found: Vec<(&str, &str)> = named
                .iter()
                .map(|(key, value)| (*key, value.as_str()))
                .collect();
            let template = if app.version.is_some() {
                Msg::SpaceDecideCheckInstalled
            } else {
                Msg::SpaceDecideCheckInstalledUnversioned
            };
            let mut lines = vec![fill_template(lang.text(template), &found)];
            if app.relation == VersionRelation::Same {
                lines.push(lang.text(Msg::SpaceDecideCheckSameVersion).to_owned());
            }
            if file
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xip"))
            {
                lines.push(fill_template(
                    lang.text(Msg::SpaceDecideCheckReload),
                    &[("{source}", "developer.apple.com")],
                ));
            }
            lines
        }
        DecideReason::Unpacked(folder) => vec![fill_template(
            lang.text(Msg::SpaceDecideCheckUnpacked),
            &[("{folder}", &shown_path(folder, ctx.home))],
        )],
        DecideReason::Duplicate(copy) => vec![fill_template(
            lang.text(Msg::SpaceDecideCheckDuplicate),
            &[("{path}", &shown_path(copy, ctx.home))],
        )],
        DecideReason::Untouched => vec![lang.text(Msg::SpaceDecideGroupReasonUntouched).to_owned()],
        DecideReason::Rest => Vec::new(),
    }
}

/// What kind of thing a file is, from its extension.
pub fn kind_text(path: &Path, is_folder: bool, ctx: ViewCtx<'_>) -> String {
    let msg = if is_folder {
        Msg::SpaceDecideKindFolder
    } else {
        let name = file_name(path).to_lowercase();
        let ends = |suffixes: &[&str]| suffixes.iter().any(|suffix| name.ends_with(suffix));
        if ends(&[".xip"]) {
            Msg::SpaceDecideKindInstallerArchive
        } else if ends(&[".dmg"]) {
            Msg::SpaceDecideKindInstallerDiskImage
        } else if ends(&[".pkg"]) {
            Msg::SpaceDecideKindInstallerPackage
        } else if ends(&[".exe", ".msi"]) {
            Msg::SpaceDecideKindInstallerWindows
        } else if ends(&[".appimage"]) {
            Msg::SpaceDecideKindAppImage
        } else if ends(&[
            ".zip", ".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tar.xz", ".tar.zst", ".7z", ".rar",
        ]) {
            Msg::SpaceDecideKindArchive
        } else if ends(&[".iso", ".vhdx", ".vmdk", ".img"]) {
            Msg::SpaceDecideKindDiskImage
        } else if ends(&[".mov", ".mp4", ".mkv", ".avi", ".webm", ".m4v"]) {
            Msg::SpaceDecideKindVideo
        } else if ends(&[".mp3", ".wav", ".flac", ".m4a", ".aac", ".ogg"]) {
            Msg::SpaceDecideKindAudio
        } else if ends(&[
            ".png", ".jpg", ".jpeg", ".gif", ".heic", ".tiff", ".webp", ".svg",
        ]) {
            Msg::SpaceDecideKindImage
        } else if ends(&[".pdf"]) {
            Msg::SpaceDecideKindPdf
        } else if ends(&[
            ".doc", ".docx", ".odt", ".xls", ".xlsx", ".ods", ".ppt", ".pptx", ".txt", ".md",
            ".pages", ".numbers",
        ]) {
            Msg::SpaceDecideKindDocument
        } else {
            Msg::SpaceDecideKindFile
        }
    };
    ctx.lang.text(msg).to_owned()
}

/// The keys the status bar shows while the pane is open.
pub fn key_hints(ctx: ViewCtx<'_>) -> Vec<(String, Msg)> {
    vec![
        ("\u{2191}\u{2193}".to_owned(), Msg::SpaceKeyChoose),
        (
            ctx.lang.text(Msg::SpaceDecideKeySpace).to_owned(),
            Msg::SpaceKeyToggle,
        ),
        ("Enter".to_owned(), Msg::SpaceKeyFold),
        (ctx.keys.label(Shortcut::Trash), Msg::SpaceKeyTrash),
        ("Tab".to_owned(), Msg::SpaceKeyNextArea),
        ("Esc".to_owned(), Msg::SpaceKeyBack),
    ]
}

/// How many files and how much the pane has chosen, for the right of the status bar.
pub fn chosen_text(pane: &DecideState, ctx: ViewCtx<'_>) -> Option<String> {
    (pane.chosen_count() > 0).then(|| {
        fill_plural(
            Msg::SpaceDecideStatusFilesChosenOne,
            Msg::SpaceDecideStatusFilesChosen,
            pane.chosen_count(),
            pane.chosen_bytes(),
            ctx.lang,
        )
    })
}
