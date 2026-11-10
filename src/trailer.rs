//! Renders the app scenes of the product trailer as single pictures and writes
//! `timeline.json` for the stage (`scripts/trailer/stage.ts`), which adds camera,
//! titles, key caps and the cross-fades. Ignored by default; run with
//! `cargo test -p ncrs trailer_frames -- --ignored`.
//!
//! Output in `target/trailer/`: `frames/<scene>-<n>.png` and `timeline.json`.
//!
//! Where the end state of a key needs the disk or a server, the key is routed
//! or noted and the state it would reach is set directly.

// reason: test code; a failing setup is the signal.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use iced::keyboard::{key::Code, key::Named, key::Physical, Key, Modifiers};

use crate::app::{route_press, App};
use crate::fs::FileEntry;
use crate::messages::{BatchRenameMessage, CommandPaletteMessage, Message, PanelSide};
use crate::palette::ThemePreset;
use crate::screenshot_tests::write_view_to;
use crate::trailer_data::{
    archive_listing, dense_compare_app, dense_search_results, dense_showcase_app,
    downloads_position, server_listing, FRAME_SIZE,
};
use ncrs_config::keymap::KeyAction;

const OUTPUT_DIRECTORY: &str = "target/trailer";

const LETTER: f32 = 0.07;

/// The area the camera aims at, in logical pixels of the window.
#[derive(Clone, Copy)]
struct Focus {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

const WHOLE_WINDOW: Focus = Focus::new(0.0, 0.0, 1920.0, 1080.0);
const LEFT_LIST: Focus = Focus::new(0.0, 300.0, 960.0, 360.0);
const RIGHT_LIST: Focus = Focus::new(960.0, 200.0, 960.0, 300.0);
const COMMAND_LIST_TOP: Focus = Focus::new(640.0, 250.0, 640.0, 400.0);
const COMMAND_LIST_FILTERED: Focus = Focus::new(640.0, 250.0, 640.0, 440.0);
const RENAME_DIALOG: Focus = Focus::new(640.0, 356.0, 640.0, 368.0);
/// The segmented colour scheme choice on the Look page, below the page label.
const COLOUR_SCHEME_SWITCH: Focus = Focus::new(1100.0, 100.0, 800.0, 90.0);

impl Focus {
    const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
}

struct Step {
    file: String,
    seconds: f32,
    key: Option<String>,
    focus: Focus,
}

struct Scene {
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    steps: Vec<Step>,
}

struct Recorder {
    app: App,
    scenes: Vec<Scene>,
    focus: Focus,
    /// The key pressed since the last picture, shown as a cap by the stage.
    key: Option<String>,
}

/// The key as a Mac shows it on a cap.
fn cap_label(key: &Key, modifiers: Modifiers) -> String {
    let mut label = String::new();
    if modifiers.control() && !modifiers.command() {
        label.push('⌃');
    }
    if modifiers.alt() {
        label.push('⌥');
    }
    if modifiers.command() {
        label.push('⌘');
    }
    if modifiers.shift() {
        label.push('⇧');
    }
    match key {
        Key::Named(Named::ArrowUp) => label.push('↑'),
        Key::Named(Named::ArrowDown) => label.push('↓'),
        Key::Named(Named::ArrowLeft) => label.push('←'),
        Key::Named(Named::ArrowRight) => label.push('→'),
        Key::Named(Named::Tab) => label.push_str("⇥ Tab"),
        Key::Named(Named::Enter) => label.push_str("↩ Enter"),
        Key::Named(Named::Escape) => label.push_str("Esc"),
        Key::Named(Named::Space) => label.push_str("Space"),
        Key::Named(Named::F1) => label.push_str("F1"),
        Key::Named(Named::F5) => label.push_str("F5"),
        Key::Named(Named::F6) => label.push_str("F6"),
        Key::Named(_) | Key::Unidentified => label.push('?'),
        Key::Character(text) => label.push_str(&text.to_uppercase()),
    }
    label
}

fn json_text(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Recorder {
    fn new() -> Self {
        let directory = Path::new(OUTPUT_DIRECTORY);
        if directory.exists() {
            std::fs::remove_dir_all(directory).expect("clear the old frames");
        }
        Self {
            app: dense_showcase_app(),
            scenes: Vec::new(),
            focus: WHOLE_WINDOW,
            key: None,
        }
    }

    fn begin_scene(&mut self, id: &'static str, title: &'static str, subtitle: &'static str) {
        self.scenes.push(Scene {
            id,
            title,
            subtitle,
            steps: Vec::new(),
        });
        self.focus = WHOLE_WINDOW;
        self.key = None;
    }

    fn fresh_app(&mut self) {
        self.app = dense_showcase_app();
    }

    fn aim(&mut self, focus: Focus) {
        self.focus = focus;
    }

    fn route(&self, key: &Key, modifiers: Modifiers) -> Message {
        route_press(
            &self.app.prompt_key_state_for_test(),
            key.clone(),
            Physical::Code(Code::Abort),
            modifiers,
            false,
        )
        .expect("the key routes to a message")
    }

    fn note_key(&mut self, key: &Key, modifiers: Modifiers) {
        self.key = Some(cap_label(key, modifiers));
    }

    /// Presses a key in the app, as the window would, and shows the result.
    fn press(&mut self, key: Key, modifiers: Modifiers, seconds: f32) {
        let message = self.route(&key, modifiers);
        self.note_key(&key, modifiers);
        self.send(message);
        self.frame(seconds);
    }

    fn plain(&mut self, named: Named, seconds: f32) {
        self.press(Key::Named(named), Modifiers::default(), seconds);
    }

    /// A key whose effect needs the disk or a server (or that a text field takes): it is
    /// noted, the caller then sets the state it would reach.
    fn press_for_fixture(&mut self, key: Key, modifiers: Modifiers) {
        self.note_key(&key, modifiers);
    }

    fn send(&mut self, message: Message) {
        drop(self.app.update(message));
    }

    fn panel(&mut self, side: PanelSide) -> &mut crate::ui::PanelState {
        self.app.panel_mut_for_test(side)
    }

    /// Shows the app as it is now for `seconds`.
    fn frame(&mut self, seconds: f32) {
        let scene = self.scenes.last().expect("a scene is open");
        let file = format!("frames/{}-{:03}.png", scene.id, scene.steps.len() + 1);
        write_view_to(
            self.app.view(),
            &format!("{OUTPUT_DIRECTORY}/frames"),
            FRAME_SIZE,
            file.trim_start_matches("frames/"),
        );
        let step = Step {
            file,
            seconds,
            key: self.key.take(),
            focus: self.focus,
        };
        self.scenes
            .last_mut()
            .expect("a scene is open")
            .steps
            .push(step);
    }

    fn type_into(&mut self, text: &str, chunk: usize, message: impl Fn(String) -> Message) {
        let letters: Vec<char> = text.chars().collect();
        let (mut end, mut shown) = (0, 0);
        while end < letters.len() {
            end = (end + chunk).min(letters.len());
            let added = end - shown;
            shown = end;
            let typed: String = letters.iter().take(end).collect();
            self.send(message(typed));
            self.frame(LETTER * added as f32);
        }
    }

    fn type_into_palette(&mut self, query: &str, chunk: usize) {
        self.type_into(query, chunk, |typed| {
            Message::CommandPalette(CommandPaletteMessage::QueryChanged(typed))
        });
    }

    fn finish(self) {
        let mut json = String::from("[\n");
        for (index, scene) in self.scenes.iter().enumerate() {
            let scene_separator = if index == 0 { "" } else { ",\n" };
            write!(
                json,
                "{scene_separator}  {{\"id\": {}, \"title\": {}, \"subtitle\": {}, \"steps\": [",
                json_text(scene.id),
                json_text(scene.title),
                json_text(scene.subtitle),
            )
            .unwrap();
            for (position, step) in scene.steps.iter().enumerate() {
                let step_separator = if position == 0 { "" } else { ",\n" };
                let key = step.key.as_deref().map_or(String::new(), |key| {
                    format!(", \"key\": {}", json_text(key))
                });
                write!(
                    json,
                    "{step_separator}    {{\"file\": {}, \"seconds\": {:.3}{key}, \"focus\": {{\"x\": {}, \"y\": {}, \"w\": {}, \"h\": {}}}}}",
                    json_text(&step.file),
                    step.seconds,
                    step.focus.x,
                    step.focus.y,
                    step.focus.w,
                    step.focus.h,
                )
                .unwrap();
            }
            json.push_str("\n  ]}");
        }
        json.push_str("\n]\n");
        std::fs::write(Path::new(OUTPUT_DIRECTORY).join("timeline.json"), json)
            .expect("timeline.json");
    }
}

fn demo_entry(folder: &Path, name: &str, is_dir: bool, size: u64) -> FileEntry {
    FileEntry {
        name: OsString::from(name),
        path: folder.join(name),
        is_dir,
        is_symlink: false,
        is_parent: name == "..",
        size,
        modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000)),
        folder_size: None,
        details: Default::default(),
    }
}

fn panels(recorder: &mut Recorder) {
    recorder.begin_scene(
        "panels",
        "Two panels. All keyboard.",
        "↑↓ to move · Tab to switch",
    );
    recorder.aim(LEFT_LIST);
    recorder.frame(0.9);
    for _ in 0..4 {
        recorder.plain(Named::ArrowDown, 0.2);
    }
    recorder.plain(Named::ArrowUp, 0.2);
    recorder.aim(RIGHT_LIST);
    recorder.plain(Named::Tab, 0.8);
    for _ in 0..3 {
        recorder.plain(Named::ArrowDown, 0.2);
    }
    recorder.frame(1.0);
}

fn command_list(recorder: &mut Recorder) {
    recorder.begin_scene(
        "f1",
        "Every command, one key away",
        "F1 · recent commands on top",
    );
    recorder.app = dense_showcase_app().with_recent_commands_for_test(&[
        KeyAction::Pack,
        KeyAction::SortByModified,
        KeyAction::Mkdir,
        KeyAction::Search,
    ]);
    recorder.frame(0.7);
    recorder.aim(COMMAND_LIST_TOP);
    recorder.plain(Named::F1, 0.8);
    recorder.aim(COMMAND_LIST_FILTERED);
    recorder.type_into_palette("co", 2);
    recorder.type_into_palette("compa", 5);
    recorder.type_into_palette("compare", 7);
    recorder.frame(0.8);
    recorder.plain(Named::Escape, 1.0);
}

fn favourites(recorder: &mut Recorder) {
    recorder.begin_scene("favourite", "Your folders on ⌘1–5", "The favourites bar");
    recorder.fresh_app();
    recorder.aim(Focus::new(0.0, 60.0, 960.0, 400.0));
    recorder.frame(0.9);
    let favourite = Key::Character("2".into());
    assert_eq!(
        recorder.route(&favourite, Modifiers::COMMAND),
        Message::OpenFavourite(1)
    );
    recorder.press_for_fixture(favourite, Modifiers::COMMAND);
    let right = recorder.panel(PanelSide::Right);
    let (path, entries) = (right.path.clone(), right.entries.clone());
    let left = recorder.panel(PanelSide::Left);
    left.path = path;
    left.entries = entries;
    left.selected = 0;
    left.selection = Default::default();
    recorder.frame(1.2);
}

fn copy(recorder: &mut Recorder) {
    recorder.begin_scene("copy", "Tag. Copy. Done.", "Space to tag · F5 to copy");
    recorder.fresh_app();
    recorder.panel(PanelSide::Left).selection = Default::default();
    recorder.aim(LEFT_LIST);
    recorder.frame(0.8);
    for _ in 0..3 {
        recorder.press(Key::Named(Named::Space), Modifiers::default(), 0.45);
    }
    recorder.frame(0.45);
    recorder.aim(Focus::new(700.0, 392.0, 520.0, 295.0));
    recorder.plain(Named::F5, 1.0);
    recorder.press_for_fixture(Key::Named(Named::Enter), Modifiers::default());
    recorder.app.show_progress_for_test(2, 3);
    recorder.frame(0.8);
    recorder.app.finish_progress_for_test();
    let copied: Vec<(String, u64)> = ["Cargo.lock", "Cargo.toml", "CHANGELOG.md"]
        .into_iter()
        .filter_map(|name| {
            let left = recorder.panel(PanelSide::Left);
            let row = left.entries.iter().find(|row| row.name == name)?;
            Some((name.to_owned(), row.size))
        })
        .collect();
    let downloads = recorder.panel(PanelSide::Right);
    let folder = downloads.path.clone();
    for (name, size) in copied {
        downloads
            .entries
            .push(demo_entry(&folder, &name, false, size));
    }
    downloads.entries.sort_by(|a, b| {
        (!a.is_parent, !a.is_dir, a.name.to_ascii_lowercase()).cmp(&(
            !b.is_parent,
            !b.is_dir,
            b.name.to_ascii_lowercase(),
        ))
    });
    recorder.aim(RIGHT_LIST);
    recorder.frame(1.2);
}

fn zip(recorder: &mut Recorder) {
    recorder.begin_scene(
        "zip",
        "Archives open like folders",
        "Enter a zip, copy out of it",
    );
    recorder.fresh_app();
    recorder.aim(RIGHT_LIST);
    recorder.plain(Named::Tab, 0.45);
    recorder.press_for_fixture(Key::Named(Named::Enter), Modifiers::default());
    let archive = PathBuf::from("/Users/alex/Downloads/backup-project.zip");
    let listing = archive_listing(&archive);
    let right = recorder.panel(PanelSide::Right);
    right.path.clone_from(&archive);
    right.archive = Some(archive);
    right.entries = listing;
    right.selected = 0;
    recorder.frame(0.8);
    recorder.plain(Named::ArrowDown, 0.2);
    recorder.plain(Named::ArrowDown, 0.2);
    recorder.frame(1.0);
}

fn batch_rename(recorder: &mut Recorder) {
    recorder.begin_scene(
        "batch-rename",
        "Rename many at once",
        "Live preview before anything changes",
    );
    recorder.fresh_app();
    recorder.aim(Focus::new(960.0, 430.0, 960.0, 260.0));
    recorder.plain(Named::Tab, 0.45);
    let first = downloads_position("holiday-001.jpg");
    recorder.panel(PanelSide::Right).selected = first;
    recorder.frame(0.45);
    for _ in 0..3 {
        recorder.press(Key::Named(Named::Space), Modifiers::default(), 0.45);
    }
    recorder.frame(0.8);
    recorder.aim(RENAME_DIALOG);
    recorder.press(Key::Named(Named::F6), Modifiers::SHIFT, 1.0);
    for pattern in [
        "Holiday {counter}{extension}",
        "Holiday {date modified} {counter}{extension}",
    ] {
        recorder.send(Message::BatchRename(BatchRenameMessage::PatternInput(
            pattern.to_owned(),
        )));
        recorder.frame(1.2);
    }
}

fn search(recorder: &mut Recorder) {
    recorder.begin_scene("search", "Find anything, fast", "Names and contents");
    recorder.fresh_app();
    recorder.frame(0.7);
    recorder.aim(Focus::new(0.0, 255.0, 960.0, 610.0));
    recorder.press(
        Key::Character("f".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
        0.3,
    );
    let mut found = dense_showcase_app();
    found.show_search_results_for_test("hello", dense_search_results());
    recorder.app = found;
    recorder.aim(Focus::new(0.0, 255.0, 960.0, 610.0));
    recorder.frame(1.0);
    recorder.aim(Focus::new(0.0, 255.0, 960.0, 610.0));
    recorder.frame(2.2);
}

fn compare(recorder: &mut Recorder) {
    recorder.begin_scene(
        "compare",
        "Compare two folders",
        "Newer, missing, different at a glance",
    );
    recorder.app = dense_showcase_app().with_recent_commands_for_test(&[KeyAction::Pack]);
    recorder.frame(0.5);
    recorder.aim(COMMAND_LIST_FILTERED);
    recorder.plain(Named::F1, 0.8);
    recorder.type_into_palette("comp", 4);
    recorder.type_into_palette("compare", 7);
    recorder.frame(0.45);
    recorder.press_for_fixture(Key::Named(Named::Enter), Modifiers::default());
    recorder.app = dense_compare_app();
    recorder.frame(1.2);
    recorder.aim(LEFT_LIST);
    recorder.frame(1.2);
}

fn sftp(recorder: &mut Recorder) {
    recorder.begin_scene(
        "sftp",
        "Servers are just folders",
        "SFTP with your ssh keys",
    );
    recorder.fresh_app();
    recorder.frame(0.5);
    let address = "sftp://alex@nas.local/home/alex";
    recorder.aim(Focus::new(0.0, 1030.0, 960.0, 50.0));
    recorder.type_into(address, 4, Message::PathFieldInput);
    recorder.frame(0.45);
    recorder.press_for_fixture(Key::Named(Named::Enter), Modifiers::default());
    recorder.send(Message::PathFieldCancel);
    let folder = PathBuf::from(address);
    let listing = server_listing(address);
    let left = recorder.panel(PanelSide::Left);
    left.path = folder;
    left.entries = listing;
    left.selected = 0;
    left.selection = Default::default();
    recorder.aim(Focus::new(0.0, 0.0, 960.0, 660.0));
    recorder.frame(0.8);
}

fn themes(recorder: &mut Recorder) {
    recorder.begin_scene("themes", "Make it yours", "F1 → Color scheme");
    let mut shown = ThemePreset::Cosmic;
    recorder.app = themed_app(shown);
    recorder.aim(WHOLE_WINDOW);
    recorder.frame(0.7);
    for preset in [ThemePreset::Dracula, ThemePreset::Teal] {
        recorder.aim(COMMAND_LIST_FILTERED);
        recorder.plain(Named::F1, 0.8);
        recorder.type_into_palette("co", 2);
        recorder.type_into_palette("color", 5);
        recorder.frame(0.45);
        recorder.press_for_fixture(Key::Named(Named::Enter), Modifiers::default());
        recorder.app = App::with_colour_scheme_row_for_test();
        recorder.app.show_theme_preset_for_test(shown);
        recorder.aim(COLOUR_SCHEME_SWITCH);
        recorder.frame(0.8);
        recorder.press_for_fixture(Key::Named(Named::ArrowRight), Modifiers::default());
        recorder.app.show_theme_preset_for_test(preset);
        recorder.frame(0.8);
        recorder.press_for_fixture(Key::Named(Named::Escape), Modifiers::default());
        shown = preset;
        recorder.app = themed_app(shown);
        recorder.aim(WHOLE_WINDOW);
        recorder.frame(1.2);
    }
}

fn themed_app(preset: ThemePreset) -> App {
    let mut app = dense_showcase_app().with_recent_commands_for_test(&[
        KeyAction::Pack,
        KeyAction::SortByModified,
        KeyAction::Mkdir,
        KeyAction::Search,
    ]);
    app.show_theme_preset_for_test(preset);
    app
}

#[test]
#[ignore = "writes target/trailer; run with `cargo test -p ncrs trailer_frames -- --ignored`"]
fn trailer_frames() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let _entered = runtime.enter();
    let mut recorder = Recorder::new();
    panels(&mut recorder);
    command_list(&mut recorder);
    favourites(&mut recorder);
    copy(&mut recorder);
    zip(&mut recorder);
    batch_rename(&mut recorder);
    search(&mut recorder);
    compare(&mut recorder);
    sftp(&mut recorder);
    themes(&mut recorder);
    recorder.finish();
}
