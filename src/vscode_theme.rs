//! The colors of a VS Code color theme, laid over a preset by `vscode = "…"`
//! in `[theme]`. Only the `colors` object counts; `tokenColors` and the rest of
//! the file are not read.

use std::collections::HashMap;
use std::path::Path;

use iced::Color;
use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};

use crate::config::read_source;
use crate::palette::{contrast_ratio, ColorRole, Palette};

/// What a role takes its color from, and which color it is drawn over when
/// the theme's color is translucent.
enum Ground {
    /// The preset's window background.
    Background,
    /// The panel, as far as the theme sets it.
    Panel,
    /// Another role, already resolved.
    Role(ColorRole),
}

/// Per role the keys to try, first one present wins.
const MAPPING: [(ColorRole, &[&str], Ground); 17] = [
    (
        ColorRole::Background,
        &[
            "sideBar.background",
            "editorGroupHeader.tabsBackground",
            "editor.background",
        ],
        Ground::Background,
    ),
    (ColorRole::Panel, &["editor.background"], Ground::Background),
    (
        ColorRole::Text,
        &["editor.foreground", "foreground"],
        Ground::Panel,
    ),
    (
        ColorRole::DimText,
        &["descriptionForeground", "editorLineNumber.foreground"],
        Ground::Panel,
    ),
    (
        ColorRole::Directory,
        &["terminal.ansiBrightWhite", "editor.foreground"],
        Ground::Panel,
    ),
    (
        ColorRole::Cursor,
        &["list.activeSelectionBackground"],
        Ground::Panel,
    ),
    (
        ColorRole::CursorText,
        &["list.activeSelectionForeground"],
        Ground::Role(ColorRole::Cursor),
    ),
    (
        ColorRole::Tagged,
        &["terminal.ansiMagenta", "list.highlightForeground"],
        Ground::Panel,
    ),
    (ColorRole::Border, &["focusBorder"], Ground::Panel),
    (
        ColorRole::Bar,
        &["titleBar.activeBackground"],
        Ground::Panel,
    ),
    (
        ColorRole::BarInactive,
        &["titleBar.inactiveBackground"],
        Ground::Panel,
    ),
    (
        ColorRole::StatusBar,
        &["statusBar.background"],
        Ground::Panel,
    ),
    (
        ColorRole::Dialog,
        &["editorWidget.background"],
        Ground::Panel,
    ),
    (
        ColorRole::Menu,
        &["menu.background", "editorWidget.background"],
        Ground::Panel,
    ),
    (
        ColorRole::Accent,
        &["terminal.ansiYellow", "textLink.foreground"],
        Ground::Panel,
    ),
    (ColorRole::Success, &["terminal.ansiGreen"], Ground::Panel),
    (
        ColorRole::Error,
        &["errorForeground", "terminal.ansiRed"],
        Ground::Panel,
    ),
];

const TITLE_TEXT_KEY: &str = "titleBar.activeForeground";
const PANEL_KEY: &str = "editor.background";

/// The colors a theme file defines, with the file(s) it was read from already
/// merged. A file that could not be used gives no colors and says why in
/// `problem`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VscodeTheme {
    pub colors: HashMap<String, Color>,
    pub problem: Option<String>,
}

/// One file as read: its own colors and the `include` it names.
struct ThemeFile {
    colors: HashMap<String, Color>,
    include: Option<String>,
}

impl VscodeTheme {
    pub fn unusable(reason: String) -> Self {
        Self {
            colors: HashMap::new(),
            problem: Some(reason),
        }
    }

    /// Reads the theme at `path` and the one file it may `include`, a path
    /// relative to its folder. The including file wins; the included file's own
    /// `include` is not followed, so no chain or cycle can run away.
    pub fn load(path: &Path) -> Self {
        let own = match read_theme_file(path) {
            Ok(file) => file,
            Err(reason) => return Self::unusable(reason),
        };
        let mut colors = HashMap::new();
        let mut problem = None;
        if let Some(name) = &own.include {
            match read_included(path, name) {
                Ok(included) => colors = included,
                Err(reason) => problem = Some(reason),
            }
        }
        colors.extend(own.colors);
        Self { colors, problem }
    }

    fn first(&self, keys: &[&str]) -> Option<Color> {
        keys.iter().find_map(|key| self.colors.get(*key).copied())
    }

    /// Sets what the theme defines on `palette`; everything else keeps the
    /// value the palette has.
    pub fn apply(&self, palette: &mut Palette) {
        let preset_background = palette.background;
        let preset_title_text = palette.title_text;
        let panel = self
            .first(&[PANEL_KEY])
            .map_or(palette.panel, |color| over(color, preset_background));
        let mut applied = Vec::new();
        for (role, keys, ground) in &MAPPING {
            let Some(color) = self.first(keys) else {
                continue;
            };
            let under = match ground {
                Ground::Background => preset_background,
                Ground::Panel => panel,
                Ground::Role(other) => palette.color(*other),
            };
            palette.set(*role, over(color, under));
            applied.push(*role);
        }

        // `set` gave the title bars the text color of the cursor row.
        palette.title_text = preset_title_text;
        if let Some(color) = self.first(&[TITLE_TEXT_KEY]) {
            palette.title_text = over(color, palette.title);
        } else if applied.contains(&ColorRole::Bar) {
            palette.title_text = most_readable(palette.title, [palette.text, palette.background]);
        }
        if applied.contains(&ColorRole::Tagged) {
            palette.tagged_text = most_readable(palette.tagged, [palette.text, palette.background]);
            palette.tagged_directory =
                most_readable(palette.tagged, [palette.directory, palette.background]);
        }
    }
}

/// The candidate with the most contrast against `ground`.
fn most_readable(ground: Color, candidates: [Color; 2]) -> Color {
    let [first, second] = candidates;
    if contrast_ratio(second, ground) > contrast_ratio(first, ground) {
        second
    } else {
        first
    }
}

/// `color` drawn over the opaque `ground`.
fn over(color: Color, ground: Color) -> Color {
    let alpha = color.a;
    Color::from_rgb(
        color.r * alpha + ground.r * (1.0 - alpha),
        color.g * alpha + ground.g * (1.0 - alpha),
        color.b * alpha + ground.b * (1.0 - alpha),
    )
}

fn read_included(theme_path: &Path, name: &str) -> Result<HashMap<String, Color>, String> {
    let target = theme_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(name);
    let same_file = matches!(
        (std::fs::canonicalize(&target), std::fs::canonicalize(theme_path)),
        (Ok(included), Ok(own)) if included == own
    );
    if same_file {
        return Err(format!("include cycle: {name:?} is this very file"));
    }
    read_theme_file(&target)
        .map(|file| file.colors)
        .map_err(|reason| format!("include {name:?}: {reason}"))
}

fn read_theme_file(path: &Path) -> Result<ThemeFile, String> {
    let source = read_source(path)
        .map_err(|err| err.reason().to_owned())?
        .ok_or_else(|| "no such file".to_owned())?;
    let parsed = parse_to_value(&source, &ParseOptions::default())
        .map_err(|err| format!("not valid JSON: {err}"))?;
    let Some(JsonValue::Object(mut root)) = parsed else {
        return Err("not a JSON object".to_owned());
    };
    let include = root.take_string("include").map(|name| name.into_owned());
    let listed = root
        .take_object("colors")
        .ok_or_else(|| "no \"colors\" object".to_owned())?;
    let colors = listed
        .into_iter()
        .filter_map(|(key, value)| match value {
            JsonValue::String(text) => parse_color(&text).map(|color| (key.into_owned(), color)),
            _ => None,
        })
        .collect();
    Ok(ThemeFile { colors, include })
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`; anything else is no color here.
fn parse_color(text: &str) -> Option<Color> {
    let digits = text.strip_prefix('#')?;
    let nibbles: Vec<u8> = digits
        .chars()
        .map(|digit| {
            let value = digit.to_digit(16)?;
            u8::try_from(value).ok()
        })
        .collect::<Option<_>>()?;
    let opaque = 255.0;
    match nibbles.as_slice() {
        [r, g, b] => Some(Color::from_rgb8(r * 17, g * 17, b * 17)),
        [r, g, b, a] => Some(Color::from_rgba8(
            r * 17,
            g * 17,
            b * 17,
            f32::from(a * 17) / opaque,
        )),
        [r1, r2, g1, g2, b1, b2] => {
            Some(Color::from_rgb8(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2))
        }
        [r1, r2, g1, g2, b1, b2, a1, a2] => Some(Color::from_rgba8(
            r1 * 16 + r2,
            g1 * 16 + g2,
            b1 * 16 + b2,
            f32::from(a1 * 16 + a2) / opaque,
        )),
        _ => None,
    }
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::ThemePreset;
    use std::path::PathBuf;

    fn write(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    fn rgb(hex: u32) -> Color {
        let channel = |shift: u32| u8::try_from((hex >> shift) & 0xFF).unwrap();
        Color::from_rgb8(channel(16), channel(8), channel(0))
    }

    fn near(found: Color, expected: Color) {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(
            close(found.r, expected.r) && close(found.g, expected.g) && close(found.b, expected.b),
            "{found:?} is not {expected:?}"
        );
    }

    /// The Cosmic preset with the theme in `json` applied.
    fn applied(json: &str) -> Palette {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "theme.json", json);
        let theme = VscodeTheme::load(&path);
        assert_eq!(theme.problem, None);
        let mut palette = ThemePreset::Cosmic.palette();
        theme.apply(&mut palette);
        palette
    }

    #[test]
    fn the_first_present_key_of_a_role_counts() {
        let only_header = applied(
            r##"{"colors": {"editorGroupHeader.tabsBackground": "#112233", "editor.background": "#445566"}}"##,
        );
        assert_eq!(only_header.background, rgb(0x112233));
        assert_eq!(only_header.panel, rgb(0x445566));
        let with_side_bar = applied(
            r##"{"colors": {"sideBar.background": "#010203", "editorGroupHeader.tabsBackground": "#112233"}}"##,
        );
        assert_eq!(with_side_bar.background, rgb(0x010203));
        let last_resort = applied(r##"{"colors": {"editor.background": "#445566"}}"##);
        assert_eq!(last_resort.background, rgb(0x445566));
    }

    #[test]
    fn a_missing_key_leaves_the_preset_value() {
        let preset = ThemePreset::Cosmic.palette();
        let palette = applied(r##"{"colors": {"editor.foreground": "#abcdef"}}"##);
        assert_eq!(palette.text, rgb(0xABCDEF));
        assert_eq!(palette.fkey_number, rgb(0xABCDEF));
        assert_eq!(palette.panel, preset.panel);
        assert_eq!(palette.error, preset.error);
        assert_eq!(palette.title_text, preset.title_text);
        assert_eq!(palette.tagged_text, preset.tagged_text);
    }

    #[test]
    fn translucent_colors_are_mixed_onto_their_ground() {
        let palette = applied(
            r##"{"colors": {"editor.background": "#000000", "list.activeSelectionBackground": "#ff000080",
               "editor.foreground": "#ffffff80", "list.activeSelectionForeground": "#00ff0080"}}"##,
        );
        let half = 128.0 / 255.0;
        near(palette.cursor, Color::from_rgb(half, 0.0, 0.0));
        near(palette.text, Color::from_rgb(half, half, half));
        near(
            palette.cursor_text,
            Color::from_rgb(half * (1.0 - half), half, 0.0),
        );
    }

    #[test]
    fn the_panel_and_the_background_are_mixed_onto_the_preset_background() {
        let preset = ThemePreset::Cosmic.palette();
        let palette = applied(r##"{"colors": {"editor.background": "#ffffff80"}}"##);
        let half = 128.0 / 255.0;
        let expected = Color::from_rgb(
            half + preset.background.r * (1.0 - half),
            half + preset.background.g * (1.0 - half),
            half + preset.background.b * (1.0 - half),
        );
        near(palette.panel, expected);
        near(palette.background, expected);
    }

    #[test]
    fn short_and_long_forms_with_and_without_alpha_are_read() {
        assert_eq!(parse_color("#fa0"), Some(rgb(0xFFAA00)));
        assert_eq!(parse_color("#123456"), Some(rgb(0x123456)));
        assert_eq!(parse_color("#fa08").map(|c| c.a), Some(136.0 / 255.0));
        assert_eq!(parse_color("#12345600").map(|c| c.a), Some(0.0));
        for not_a_color in [
            "", "#", "#12", "#12345", "red", "123456", "#gg0000", "#12345é",
        ] {
            assert_eq!(parse_color(not_a_color), None, "{not_a_color}");
        }
    }

    #[test]
    fn an_invalid_value_skips_only_its_key() {
        let palette = applied(
            r##"{"colors": {"editor.foreground": "tomato", "foreground": "#102030", "terminal.ansiGreen": 5}}"##,
        );
        assert_eq!(palette.text, rgb(0x102030));
        assert_eq!(palette.success, ThemePreset::Cosmic.palette().success);
    }

    #[test]
    fn comments_and_trailing_commas_are_fine() {
        let palette = applied(
            "// a theme\n{\n  /* block */ \"name\": \"x\",\n  \"tokenColors\": [{\"settings\": {}},],\n  \"colors\": {\n    \"foreground\": \"#102030\", // text\n  },\n}\n",
        );
        assert_eq!(palette.text, rgb(0x102030));
    }

    #[test]
    fn the_including_file_overrides_the_included_one() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir,
            "base.json",
            r##"{"include": "ignored.json", "colors": {"foreground": "#111111", "terminal.ansiRed": "#222222"}}"##,
        );
        write(
            &dir,
            "ignored.json",
            r##"{"colors": {"terminal.ansiGreen": "#333333"}}"##,
        );
        let path = write(
            &dir,
            "themes/own.json",
            r##"{"include": "../base.json", "colors": {"foreground": "#444444"}}"##,
        );
        let theme = VscodeTheme::load(&path);
        assert_eq!(theme.problem, None);
        let mut palette = ThemePreset::Cosmic.palette();
        theme.apply(&mut palette);
        assert_eq!(palette.text, rgb(0x444444));
        assert_eq!(palette.error, rgb(0x222222));
        assert_eq!(palette.success, ThemePreset::Cosmic.palette().success);
    }

    #[test]
    fn an_include_of_the_file_itself_is_a_cycle_and_its_own_colors_stay() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            "t.json",
            r##"{"include": "./t.json", "colors": {"foreground": "#444444"}}"##,
        );
        let theme = VscodeTheme::load(&path);
        assert!(theme.problem.as_deref().unwrap().contains("include cycle"));
        let mut palette = ThemePreset::Cosmic.palette();
        theme.apply(&mut palette);
        assert_eq!(palette.text, rgb(0x444444));
    }

    #[test]
    fn two_files_including_each_other_end_after_one_level() {
        let dir = tempfile::tempdir().unwrap();
        let first = write(
            &dir,
            "a.json",
            r##"{"include": "b.json", "colors": {"foreground": "#111111"}}"##,
        );
        write(
            &dir,
            "b.json",
            r##"{"include": "a.json", "colors": {"foreground": "#222222", "terminal.ansiRed": "#333333"}}"##,
        );
        let theme = VscodeTheme::load(&first);
        assert_eq!(theme.problem, None);
        let mut palette = ThemePreset::Cosmic.palette();
        theme.apply(&mut palette);
        assert_eq!(palette.text, rgb(0x111111));
        assert_eq!(palette.error, rgb(0x333333));
    }

    #[test]
    fn a_missing_include_keeps_the_colors_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            "t.json",
            r##"{"include": "nowhere.json", "colors": {"foreground": "#444444"}}"##,
        );
        let theme = VscodeTheme::load(&path);
        assert!(theme.problem.as_deref().unwrap().contains("nowhere.json"));
        assert_eq!(theme.first(&["foreground"]), Some(rgb(0x444444)));
    }

    #[test]
    fn a_file_that_cannot_be_used_gives_a_reason_and_no_colors() {
        let dir = tempfile::tempdir().unwrap();
        let big = format!(
            "{{\"colors\": {{}}, \"pad\": \"{}\"}}",
            "x".repeat(1024 * 1024)
        );
        let cases = [
            ("missing.json", None, "no such file"),
            ("broken.json", Some("{ \"colors\": "), "not valid JSON"),
            ("array.json", Some("[]"), "not a JSON object"),
            ("nocolors.json", Some("{}"), "no \"colors\" object"),
            (
                "colorlist.json",
                Some("{\"colors\": []}"),
                "no \"colors\" object",
            ),
            ("big.json", Some(big.as_str()), "larger than"),
        ];
        for (name, text, reason) in cases {
            let path = match text {
                Some(content) => write(&dir, name, content),
                None => dir.path().join(name),
            };
            let theme = VscodeTheme::load(&path);
            assert!(
                theme.problem.as_deref().unwrap().contains(reason),
                "{name}: {:?}",
                theme.problem
            );
            let mut palette = ThemePreset::Cosmic.palette();
            theme.apply(&mut palette);
            assert_eq!(palette, ThemePreset::Cosmic.palette(), "{name}");
        }
        let folder = dir.path().join("folder.json");
        std::fs::create_dir(&folder).unwrap();
        assert!(VscodeTheme::load(&folder)
            .problem
            .unwrap()
            .contains("not a regular file"));
        let bytes = write(&dir, "bytes.json", "");
        std::fs::write(&bytes, [b'{', 0xFF, 0xFE]).unwrap();
        assert!(VscodeTheme::load(&bytes).problem.unwrap().contains("UTF-8"));
    }

    #[test]
    fn the_title_text_follows_its_key_or_the_more_readable_color() {
        let keyed = applied(
            r##"{"colors": {"titleBar.activeBackground": "#202020", "titleBar.activeForeground": "#abcdef"}}"##,
        );
        assert_eq!(keyed.title_text, rgb(0xABCDEF));
        let by_contrast = applied(
            r##"{"colors": {"titleBar.activeBackground": "#202020", "editor.foreground": "#eeeeee", "sideBar.background": "#101010"}}"##,
        );
        assert_eq!(by_contrast.title_text, rgb(0xEEEEEE));
        let by_contrast_on_light = applied(
            r##"{"colors": {"titleBar.activeBackground": "#f0f0f0", "editor.foreground": "#eeeeee", "sideBar.background": "#101010"}}"##,
        );
        assert_eq!(by_contrast_on_light.title_text, rgb(0x101010));
        let untouched = applied(r##"{"colors": {"list.activeSelectionForeground": "#abcdef"}}"##);
        assert_eq!(
            untouched.title_text,
            ThemePreset::Cosmic.palette().title_text
        );
        assert_eq!(untouched.cursor_text, rgb(0xABCDEF));
    }

    #[test]
    fn tagged_rows_get_the_more_readable_of_the_text_and_the_background() {
        let dark_tag = applied(
            r##"{"colors": {"terminal.ansiMagenta": "#282828", "editor.foreground": "#303030",
               "terminal.ansiBrightWhite": "#ededed", "sideBar.background": "#ffffff"}}"##,
        );
        assert_eq!(dark_tag.tagged_text, rgb(0xFFFFFF));
        assert_eq!(dark_tag.tagged_directory, rgb(0xFFFFFF));
        let light_tag = applied(
            r##"{"colors": {"terminal.ansiMagenta": "#f0f0f0", "editor.foreground": "#101010",
               "terminal.ansiBrightWhite": "#202020", "sideBar.background": "#fafafa"}}"##,
        );
        assert_eq!(light_tag.tagged_text, rgb(0x101010));
        assert_eq!(light_tag.tagged_directory, rgb(0x202020));
    }

    mod in_the_config {
        use super::*;
        use crate::config::load;

        fn config_with(dir: &tempfile::TempDir, text: &str) -> crate::config::Config {
            load(&write(dir, "config.toml", text)).unwrap()
        }

        #[test]
        fn the_order_is_preset_then_theme_then_roles() {
            let dir = tempfile::tempdir().unwrap();
            write(
                &dir,
                "themes/t.json",
                r##"{"colors": {"editor.background": "#102030", "foreground": "#405060"}}"##,
            );
            let config = config_with(
                &dir,
                "[theme]\ntext = \"#010203\"\nvscode = \"themes/t.json\"\npreset = \"classic\"\n",
            );
            let classic = ThemePreset::Classic.palette();
            assert_eq!(config.vscode_warning, None);
            assert_eq!(config.palette.panel, rgb(0x102030));
            assert_eq!(config.palette.text, rgb(0x010203));
            assert_eq!(config.palette.error, classic.error);
        }

        #[test]
        fn a_wrong_path_warns_and_keeps_the_look_and_the_rest_of_the_config() {
            let dir = tempfile::tempdir().unwrap();
            let config = config_with(
                &dir,
                "[theme]\npreset = \"classic\"\nvscode = \"themes/gone.json\"\ndirectory = \"#fff\"\n\n\
                 [modules.filing]\nenabled = true\n",
            );
            let warning = config.vscode_warning.unwrap();
            assert_eq!(warning.line, Some(3));
            assert!(
                warning.reason.contains("no such file"),
                "{}",
                warning.reason
            );
            assert_eq!(warning.theme, dir.path().join("themes/gone.json"));
            assert_eq!(config.palette.panel, ThemePreset::Classic.palette().panel);
            assert_eq!(config.palette.directory, rgb(0xFFFFFF));
            assert!(config.filing.enabled);
        }
    }
}
