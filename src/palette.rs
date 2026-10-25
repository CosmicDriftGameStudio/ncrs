//! The colors of the window and how the `[theme]` section of the config
//! changes them.
//!
//! A `Palette` has one slot per place a color is used. The config speaks in
//! `ColorRole`s instead: a role is something a user can name ("the text"), and
//! sets every slot that has to move with it, so one line cannot leave the
//! window half recolored.

use std::ops::Range;

use iced::Color;

use crate::config::ThemeSection;
use crate::vscode_theme::VscodeTheme;

/// Below this ratio (WCAG calls 3:1 the floor for large or bold text) a
/// pairing is reported, not refused: the user's choice stands.
const READABLE_CONTRAST: f32 = 3.0;

/// The key of `[theme]` that picks the base palette.
const PRESET_KEY: &str = "preset";
/// Like `preset`, not a role: a VS Code theme file laid over the preset.
pub const VSCODE_KEY: &str = "vscode";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Color,
    pub panel: Color,
    pub text: Color,
    pub dim_text: Color,
    pub directory: Color,
    pub cursor: Color,
    pub cursor_text: Color,
    pub tagged: Color,
    pub tagged_inactive: Color,
    /// Names on a tagged row of the active panel, when the cursor is elsewhere.
    pub tagged_text: Color,
    pub tagged_directory: Color,
    /// The star of a tagged row of the active panel, when the cursor is elsewhere.
    pub tagged_mark: Color,
    pub border: Color,
    pub border_inactive: Color,
    pub title: Color,
    /// Text on the active panel title.
    pub title_text: Color,
    pub title_inactive: Color,
    pub status_bar: Color,
    pub dialog: Color,
    /// Behind the context menu: darker than the dialog box so the dimmed texts
    /// on it stay legible.
    pub menu: Color,
    pub menu_shortcut: Color,
    pub menu_shortcut_on_cursor: Color,
    pub menu_disabled: Color,
    pub accent: Color,
    /// Confirmations, drawn on the status bar.
    pub success: Color,
    pub error: Color,
    /// The digit of a function key slot, on the window background.
    pub fkey_number: Color,
    /// The label box behind a function key's name.
    pub fkey_label: Color,
    pub fkey_label_text: Color,
}

impl Default for Palette {
    fn default() -> Self {
        ThemePreset::Cosmic.palette()
    }
}

/// A complete palette to start from. `[theme]` roles are applied on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePreset {
    /// The look without a `[theme]` section.
    Cosmic,
    /// The earlier dark teal look, still called `"default"` in the config.
    Teal,
    Classic,
    Dracula,
}

impl ThemePreset {
    pub const ALL: [ThemePreset; 4] = [Self::Cosmic, Self::Teal, Self::Classic, Self::Dracula];

    pub fn name(self) -> &'static str {
        match self {
            Self::Cosmic => "cosmic",
            Self::Teal => "default",
            Self::Classic => "classic",
            Self::Dracula => "dracula",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.name() == name)
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Cosmic => Palette {
                background: Color::from_rgb8(0x14, 0x13, 0x18),
                panel: Color::from_rgb8(0x1A, 0x19, 0x20),
                text: Color::from_rgb8(0xCF, 0xCD, 0xD6),
                dim_text: Color::from_rgb8(0x85, 0x82, 0x8F),
                directory: Color::from_rgb8(0xF0, 0xEE, 0xF5),
                cursor: Color::from_rgb8(0x7F, 0xA8, 0x8A),
                cursor_text: Color::from_rgb8(0x14, 0x13, 0x18),
                tagged: Color::from_rgb8(0xB3, 0x9D, 0xDB),
                tagged_inactive: Color::from_rgb8(0x3D, 0x36, 0x50),
                tagged_text: Color::from_rgb8(0x14, 0x13, 0x18),
                tagged_directory: Color::from_rgb8(0x14, 0x13, 0x18),
                tagged_mark: Color::from_rgb8(0x14, 0x13, 0x18),
                border: Color::from_rgb8(0x72, 0x6E, 0x8A),
                border_inactive: Color::from_rgb8(0x1E, 0x1D, 0x25),
                title: Color::from_rgb8(0x24, 0x22, 0x2D),
                title_text: Color::from_rgb8(0xCF, 0xCD, 0xD6),
                title_inactive: Color::from_rgb8(0x1E, 0x1D, 0x25),
                status_bar: Color::from_rgb8(0x18, 0x17, 0x1D),
                dialog: Color::from_rgb8(0x24, 0x22, 0x2D),
                menu: Color::from_rgb8(0x24, 0x22, 0x2D),
                menu_shortcut: Color::from_rgb8(0xD4, 0xA3, 0x73),
                menu_shortcut_on_cursor: Color::from_rgb8(0x14, 0x13, 0x18),
                menu_disabled: Color::from_rgb8(0x85, 0x82, 0x8F),
                accent: Color::from_rgb8(0xD4, 0xA3, 0x73),
                success: Color::from_rgb8(0x8F, 0xB9, 0x96),
                error: Color::from_rgb8(0xD9, 0x7A, 0x7A),
                fkey_number: Color::from_rgb8(0xCF, 0xCD, 0xD6),
                fkey_label: Color::from_rgb8(0x7F, 0xA8, 0x8A),
                fkey_label_text: Color::from_rgb8(0x14, 0x13, 0x18),
            },
            Self::Teal => Palette {
                background: Color::from_rgb(0.035, 0.047, 0.098),
                panel: Color::from_rgb(0.047, 0.086, 0.200),
                text: Color::from_rgb(0.800, 0.840, 0.940),
                dim_text: Color::from_rgb(0.450, 0.500, 0.640),
                directory: Color::from_rgb(1.000, 1.000, 1.000),
                cursor: Color::from_rgb(0.180, 0.690, 0.760),
                cursor_text: Color::from_rgb(0.020, 0.040, 0.090),
                tagged: Color::from_rgb(0.180, 0.690, 0.760),
                tagged_inactive: Color::from_rgb(0.130, 0.210, 0.360),
                tagged_text: Color::from_rgb(0.800, 0.840, 0.940),
                tagged_directory: Color::from_rgb(1.000, 1.000, 1.000),
                tagged_mark: Color::from_rgb(1.000, 0.850, 0.300),
                border: Color::from_rgb(0.310, 0.840, 0.910),
                border_inactive: Color::from_rgb(0.165, 0.210, 0.390),
                title: Color::from_rgb(0.110, 0.420, 0.520),
                title_text: Color::from_rgb(0.020, 0.040, 0.090),
                title_inactive: Color::from_rgb(0.070, 0.125, 0.270),
                status_bar: Color::from_rgb(0.070, 0.125, 0.270),
                dialog: Color::from_rgb(0.110, 0.420, 0.520),
                menu: Color::from_rgb(0.070, 0.125, 0.270),
                menu_shortcut: Color::from_rgb(0.560, 0.800, 0.880),
                menu_shortcut_on_cursor: Color::from_rgb(0.040, 0.100, 0.180),
                menu_disabled: Color::from_rgb(0.560, 0.610, 0.730),
                accent: Color::from_rgb(1.000, 0.850, 0.300),
                success: Color::from_rgb(0.600, 1.000, 0.650),
                error: Color::from_rgb(1.000, 0.450, 0.450),
                fkey_number: Color::from_rgb(0.850, 0.870, 0.920),
                fkey_label: Color::from_rgb(0.180, 0.690, 0.760),
                fkey_label_text: Color::from_rgb(0.020, 0.040, 0.090),
            },
            Self::Classic => Palette {
                background: Color::from_rgb8(0x00, 0x00, 0x00),
                panel: Color::from_rgb8(0x00, 0x00, 0xAA),
                text: Color::from_rgb8(0x55, 0xFF, 0xFF),
                dim_text: Color::from_rgb8(0x00, 0xAA, 0xAA),
                directory: Color::from_rgb8(0xFF, 0xFF, 0xFF),
                cursor: Color::from_rgb8(0x00, 0xAA, 0xAA),
                cursor_text: Color::from_rgb8(0x00, 0x00, 0x00),
                tagged: Color::from_rgb8(0x55, 0x55, 0xFF),
                tagged_inactive: Color::from_rgb8(0x00, 0x00, 0x80),
                tagged_text: Color::from_rgb8(0x55, 0xFF, 0xFF),
                tagged_directory: Color::from_rgb8(0xFF, 0xFF, 0xFF),
                tagged_mark: Color::from_rgb8(0xFF, 0xFF, 0x55),
                border: Color::from_rgb8(0x55, 0xFF, 0xFF),
                border_inactive: Color::from_rgb8(0x00, 0xAA, 0xAA),
                title: Color::from_rgb8(0x00, 0xAA, 0xAA),
                title_text: Color::from_rgb8(0x00, 0x00, 0x00),
                title_inactive: Color::from_rgb8(0x00, 0x00, 0xAA),
                status_bar: Color::from_rgb8(0x00, 0x00, 0x00),
                dialog: Color::from_rgb8(0x00, 0x00, 0x80),
                menu: Color::from_rgb8(0x00, 0x00, 0x80),
                menu_shortcut: Color::from_rgb8(0x00, 0xAA, 0xAA),
                menu_shortcut_on_cursor: Color::from_rgb8(0x00, 0x00, 0x00),
                menu_disabled: Color::from_rgb8(0x80, 0x80, 0x80),
                accent: Color::from_rgb8(0xFF, 0xFF, 0x55),
                success: Color::from_rgb8(0x55, 0xFF, 0x55),
                error: Color::from_rgb8(0xFF, 0x55, 0x55),
                fkey_number: Color::from_rgb8(0xFF, 0xFF, 0xFF),
                fkey_label: Color::from_rgb8(0x00, 0xAA, 0xAA),
                fkey_label_text: Color::from_rgb8(0x00, 0x00, 0x00),
            },
            // Colors from the Dracula theme (draculatheme.com), MIT license.
            Self::Dracula => Palette {
                background: Color::from_rgb8(0x21, 0x22, 0x2C),
                panel: Color::from_rgb8(0x28, 0x2A, 0x36),
                text: Color::from_rgb8(0xF8, 0xF8, 0xF2),
                dim_text: Color::from_rgb8(0x62, 0x72, 0xA4),
                directory: Color::from_rgb8(0x8B, 0xE9, 0xFD),
                cursor: Color::from_rgb8(0x44, 0x47, 0x5A),
                cursor_text: Color::from_rgb8(0xF8, 0xF8, 0xF2),
                tagged: Color::from_rgb8(0xFF, 0x79, 0xC6),
                tagged_inactive: Color::from_rgb8(0x6E, 0x3F, 0x63),
                tagged_text: Color::from_rgb8(0x28, 0x2A, 0x36),
                tagged_directory: Color::from_rgb8(0x28, 0x2A, 0x36),
                tagged_mark: Color::from_rgb8(0x28, 0x2A, 0x36),
                border: Color::from_rgb8(0xBD, 0x93, 0xF9),
                border_inactive: Color::from_rgb8(0x44, 0x47, 0x5A),
                title: Color::from_rgb8(0x44, 0x47, 0x5A),
                title_text: Color::from_rgb8(0xF8, 0xF8, 0xF2),
                title_inactive: Color::from_rgb8(0x21, 0x22, 0x2C),
                status_bar: Color::from_rgb8(0x21, 0x22, 0x2C),
                dialog: Color::from_rgb8(0x21, 0x22, 0x2C),
                menu: Color::from_rgb8(0x21, 0x22, 0x2C),
                menu_shortcut: Color::from_rgb8(0xFF, 0xB8, 0x6C),
                menu_shortcut_on_cursor: Color::from_rgb8(0xFF, 0xB8, 0x6C),
                menu_disabled: Color::from_rgb8(0x62, 0x72, 0xA4),
                accent: Color::from_rgb8(0xF1, 0xFA, 0x8C),
                success: Color::from_rgb8(0x50, 0xFA, 0x7B),
                error: Color::from_rgb8(0xFF, 0x55, 0x55),
                fkey_number: Color::from_rgb8(0xF8, 0xF8, 0xF2),
                fkey_label: Color::from_rgb8(0x44, 0x47, 0x5A),
                fkey_label_text: Color::from_rgb8(0xF8, 0xF8, 0xF2),
            },
        }
    }
}

macro_rules! color_roles {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// Something in the window a user can recolor. The name is what
        /// `[theme]` calls it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum ColorRole {
            $($variant),+
        }

        impl ColorRole {
            #[cfg(test)]
            pub const ALL: &'static [ColorRole] = &[$(Self::$variant),+];

            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

color_roles! {
    Background => "background",
    Panel => "panel",
    Text => "text",
    DimText => "dim_text",
    Directory => "directory",
    Cursor => "cursor",
    CursorText => "cursor_text",
    Tagged => "tagged",
    Border => "border",
    Bar => "bar",
    BarInactive => "bar_inactive",
    StatusBar => "status_bar",
    Dialog => "dialog",
    Menu => "menu",
    Accent => "accent",
    Success => "success",
    Error => "error",
}

impl Palette {
    /// Sets every slot that belongs to `role`.
    pub fn set(&mut self, role: ColorRole, color: Color) {
        match role {
            ColorRole::Background => self.background = color,
            ColorRole::Panel => self.panel = color,
            ColorRole::Text => {
                self.text = color;
                self.fkey_number = color;
            }
            ColorRole::DimText => {
                self.dim_text = color;
                self.menu_disabled = color;
            }
            ColorRole::Directory => self.directory = color,
            ColorRole::Cursor => {
                self.cursor = color;
                self.fkey_label = color;
            }
            ColorRole::CursorText => {
                self.cursor_text = color;
                self.title_text = color;
                self.fkey_label_text = color;
                self.menu_shortcut_on_cursor = color;
            }
            ColorRole::Tagged => {
                self.tagged = color;
                self.tagged_inactive = color;
            }
            ColorRole::Border => self.border = color,
            ColorRole::Bar => self.title = color,
            ColorRole::BarInactive => {
                self.title_inactive = color;
                self.border_inactive = color;
            }
            ColorRole::StatusBar => self.status_bar = color,
            ColorRole::Dialog => self.dialog = color,
            ColorRole::Menu => self.menu = color,
            ColorRole::Accent => {
                self.accent = color;
                self.menu_shortcut = color;
            }
            ColorRole::Success => self.success = color,
            ColorRole::Error => self.error = color,
        }
    }

    /// The main slot of `role`, the one its contrast is judged by.
    pub fn color(&self, role: ColorRole) -> Color {
        match role {
            ColorRole::Background => self.background,
            ColorRole::Panel => self.panel,
            ColorRole::Text => self.text,
            ColorRole::DimText => self.dim_text,
            ColorRole::Directory => self.directory,
            ColorRole::Cursor => self.cursor,
            ColorRole::CursorText => self.cursor_text,
            ColorRole::Tagged => self.tagged,
            ColorRole::Border => self.border,
            ColorRole::Bar => self.title,
            ColorRole::BarInactive => self.title_inactive,
            ColorRole::StatusBar => self.status_bar,
            ColorRole::Dialog => self.dialog,
            ColorRole::Menu => self.menu,
            ColorRole::Accent => self.accent,
            ColorRole::Success => self.success,
            ColorRole::Error => self.error,
        }
    }

    /// The pairing of text and ground with the least contrast, when it is below
    /// `READABLE_CONTRAST`.
    ///
    /// Text on the cursor and tagged rows and the error color on the dialog are
    /// not judged: the built-in palette is already weak there.
    pub fn weakest_contrast(&self) -> Option<LowContrast> {
        const PAIRS: [(ColorRole, ColorRole); 8] = [
            (ColorRole::Text, ColorRole::Panel),
            (ColorRole::Directory, ColorRole::Panel),
            (ColorRole::CursorText, ColorRole::Cursor),
            (ColorRole::Text, ColorRole::StatusBar),
            (ColorRole::Text, ColorRole::Dialog),
            (ColorRole::Text, ColorRole::Menu),
            (ColorRole::Error, ColorRole::StatusBar),
            (ColorRole::Success, ColorRole::StatusBar),
        ];
        PAIRS
            .into_iter()
            .map(|(foreground, background)| LowContrast {
                foreground,
                background,
                ratio: contrast_ratio(self.color(foreground), self.color(background)),
            })
            .filter(|pair| pair.ratio < READABLE_CONTRAST)
            .min_by(|a, b| a.ratio.total_cmp(&b.ratio))
    }

    /// The presets with their own text colors on tagged rows and bars (Cosmic,
    /// Dracula) would turn unreadable when a role moves the fill or the color
    /// the slot used to follow. Then the slot takes the more readable of its
    /// value before the roles and the color it used to follow. Teal and Classic
    /// never differ from that color and stay as they are.
    fn keep_readable(&mut self, preset: &Palette, before: &Palette, set: &[ColorRole]) {
        let touched = |a: ColorRole, b: ColorRole| set.contains(&a) || set.contains(&b);
        if preset.tagged_text != preset.text && touched(ColorRole::Tagged, ColorRole::Text) {
            self.tagged_text = most_readable(self.tagged, [before.tagged_text, self.text]);
        }
        if preset.tagged_directory != preset.directory
            && touched(ColorRole::Tagged, ColorRole::Directory)
        {
            self.tagged_directory =
                most_readable(self.tagged, [before.tagged_directory, self.directory]);
        }
        if preset.tagged_mark != preset.accent && touched(ColorRole::Tagged, ColorRole::Accent) {
            self.tagged_mark = most_readable(self.tagged, [before.tagged_mark, self.accent]);
        }
        if preset.title_text != preset.cursor_text && touched(ColorRole::Bar, ColorRole::CursorText)
        {
            self.title_text = most_readable(self.title, [before.title_text, self.cursor_text]);
        }
    }

    /// The palette `[theme]` describes: the preset (wherever it stands in the
    /// section), then the VS Code theme read from its `vscode` entry, then the
    /// roles in the order written.
    pub fn from_section(
        section: &ThemeSection,
        vscode: Option<&VscodeTheme>,
    ) -> Result<ConfiguredPalette, ThemeError> {
        let mut preset_span = None;
        let mut vscode_span = None;
        let mut preset = ThemePreset::Cosmic;
        for entry in &section.entries {
            if entry.key.as_ref() == PRESET_KEY {
                let name = entry.value.as_ref();
                preset = ThemePreset::from_name(name).ok_or_else(|| ThemeError {
                    span: entry.value.span(),
                    reason: format!(
                        "unknown preset {name:?}, expected {}",
                        ThemePreset::ALL
                            .map(|known| format!("{:?}", known.name()))
                            .join(" or ")
                    ),
                })?;
                preset_span = Some(entry.value.span());
            }
        }

        let mut palette = preset.palette();
        if let Some(theme) = vscode {
            theme.apply(&mut palette);
        }
        let before_roles = palette;
        let mut role_spans = Vec::new();
        for entry in &section.entries {
            let key = entry.key.as_ref();
            if key == PRESET_KEY {
                continue;
            }
            if key == VSCODE_KEY {
                vscode_span = Some(entry.value.span());
                continue;
            }
            let role = ColorRole::from_name(key).ok_or_else(|| ThemeError {
                span: entry.key.span(),
                reason: format!("unknown color role {key:?}"),
            })?;
            let color = parse_color(entry.value.as_ref()).map_err(|reason| ThemeError {
                span: entry.value.span(),
                reason,
            })?;
            palette.set(role, color);
            role_spans.push((role, entry.value.span()));
        }
        let set_roles: Vec<ColorRole> = role_spans.iter().map(|(role, _)| *role).collect();
        palette.keep_readable(&preset.palette(), &before_roles, &set_roles);
        Ok(ConfiguredPalette {
            palette,
            preset_span,
            vscode_span,
            role_spans,
        })
    }
}

/// Why `[theme]` cannot be applied, and where in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    pub span: Range<usize>,
    pub reason: String,
}

/// A palette built from `[theme]`, with where in the file its parts came from.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfiguredPalette {
    pub palette: Palette,
    pub preset_span: Option<Range<usize>>,
    pub vscode_span: Option<Range<usize>>,
    pub role_spans: Vec<(ColorRole, Range<usize>)>,
}

impl ConfiguredPalette {
    pub fn span_of(&self, role: ColorRole) -> Option<Range<usize>> {
        self.role_spans
            .iter()
            .find(|(set, _)| *set == role)
            .map(|(_, span)| span.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LowContrast {
    pub foreground: ColorRole,
    pub background: ColorRole,
    pub ratio: f32,
}

/// `#rrggbb` or `#rgb`. Alpha is refused: a translucent panel would show the
/// window behind it, which nothing here is drawn to handle.
/// The candidate with the most contrast against `ground`.
pub fn most_readable(ground: Color, candidates: [Color; 2]) -> Color {
    let [first, second] = candidates;
    if contrast_ratio(second, ground) > contrast_ratio(first, ground) {
        second
    } else {
        first
    }
}

fn parse_color(text: &str) -> Result<Color, String> {
    let not_a_color = || format!("{text:?} is not a color, expected \"#rrggbb\" or \"#rgb\"");
    let digits = text.strip_prefix('#').ok_or_else(not_a_color)?;
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(not_a_color());
    }
    let values: Vec<u8> = digits
        .chars()
        .filter_map(|digit| digit.to_digit(16))
        .filter_map(|value| u8::try_from(value).ok())
        .collect();
    match values.as_slice() {
        [r, g, b] => Ok(Color::from_rgb8(r * 17, g * 17, b * 17)),
        [r1, r2, g1, g2, b1, b2] => Ok(Color::from_rgb8(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2)),
        _ => Err(not_a_color()),
    }
}

fn linear(channel: f32) -> f32 {
    if channel <= 0.03928 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(color: Color) -> f32 {
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

/// WCAG contrast ratio, 1.0 (same color) to 21.0 (black on white).
pub fn contrast_ratio(a: Color, b: Color) -> f32 {
    let (first, second) = (luminance(a), luminance(b));
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
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
    use crate::config::{load, ConfigError};
    use std::path::PathBuf;

    fn slots(palette: &Palette) -> [(&'static str, Color); 29] {
        [
            ("background", palette.background),
            ("panel", palette.panel),
            ("text", palette.text),
            ("dim_text", palette.dim_text),
            ("directory", palette.directory),
            ("cursor", palette.cursor),
            ("cursor_text", palette.cursor_text),
            ("tagged", palette.tagged),
            ("tagged_inactive", palette.tagged_inactive),
            ("tagged_text", palette.tagged_text),
            ("tagged_directory", palette.tagged_directory),
            ("tagged_mark", palette.tagged_mark),
            ("border", palette.border),
            ("border_inactive", palette.border_inactive),
            ("title", palette.title),
            ("title_text", palette.title_text),
            ("title_inactive", palette.title_inactive),
            ("status_bar", palette.status_bar),
            ("dialog", palette.dialog),
            ("menu", palette.menu),
            ("menu_shortcut", palette.menu_shortcut),
            ("menu_shortcut_on_cursor", palette.menu_shortcut_on_cursor),
            ("menu_disabled", palette.menu_disabled),
            ("accent", palette.accent),
            ("success", palette.success),
            ("error", palette.error),
            ("fkey_number", palette.fkey_number),
            ("fkey_label", palette.fkey_label),
            ("fkey_label_text", palette.fkey_label_text),
        ]
    }

    fn write(dir: &tempfile::TempDir, content: &str) -> PathBuf {
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        path
    }

    fn load_text(content: &str) -> crate::config::Config {
        let dir = tempfile::tempdir().unwrap();
        load(&write(&dir, content)).unwrap()
    }

    fn error_of(content: &str) -> (Option<usize>, String) {
        let dir = tempfile::tempdir().unwrap();
        let err = load(&write(&dir, content)).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
        (err.line(), err.reason().to_owned())
    }

    fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color::from_rgb(r, g, b)
    }

    #[test]
    fn without_a_theme_section_the_cosmic_preset_applies() {
        for text in ["", "[open]\nview = [\"less\"]\n", "[theme]\n"] {
            let config = load_text(text);
            assert_eq!(config.palette, ThemePreset::Cosmic.palette(), "{text:?}");
            assert_eq!(config.contrast_warning, None);
        }
    }

    #[test]
    fn the_name_default_picks_the_teal_look() {
        let config = load_text("[theme]\npreset = \"default\"\n");
        assert_eq!(config.palette, ThemePreset::Teal.palette());
    }

    #[test]
    fn every_slot_of_the_teal_preset_keeps_its_look() {
        let expected = [
            ("background", rgb(0.035, 0.047, 0.098)),
            ("panel", rgb(0.047, 0.086, 0.200)),
            ("text", rgb(0.800, 0.840, 0.940)),
            ("dim_text", rgb(0.450, 0.500, 0.640)),
            ("directory", rgb(1.000, 1.000, 1.000)),
            ("cursor", rgb(0.180, 0.690, 0.760)),
            ("cursor_text", rgb(0.020, 0.040, 0.090)),
            ("tagged", rgb(0.180, 0.690, 0.760)),
            ("tagged_inactive", rgb(0.130, 0.210, 0.360)),
            ("tagged_text", rgb(0.800, 0.840, 0.940)),
            ("tagged_directory", rgb(1.000, 1.000, 1.000)),
            ("tagged_mark", rgb(1.000, 0.850, 0.300)),
            ("border", rgb(0.310, 0.840, 0.910)),
            ("border_inactive", rgb(0.165, 0.210, 0.390)),
            ("title", rgb(0.110, 0.420, 0.520)),
            ("title_text", rgb(0.020, 0.040, 0.090)),
            ("title_inactive", rgb(0.070, 0.125, 0.270)),
            ("status_bar", rgb(0.070, 0.125, 0.270)),
            ("dialog", rgb(0.110, 0.420, 0.520)),
            ("menu", rgb(0.070, 0.125, 0.270)),
            ("menu_shortcut", rgb(0.560, 0.800, 0.880)),
            ("menu_shortcut_on_cursor", rgb(0.040, 0.100, 0.180)),
            ("menu_disabled", rgb(0.560, 0.610, 0.730)),
            ("accent", rgb(1.000, 0.850, 0.300)),
            ("success", rgb(0.600, 1.000, 0.650)),
            ("error", rgb(1.000, 0.450, 0.450)),
            ("fkey_number", rgb(0.850, 0.870, 0.920)),
            ("fkey_label", rgb(0.180, 0.690, 0.760)),
            ("fkey_label_text", rgb(0.020, 0.040, 0.090)),
        ];
        assert_eq!(slots(&ThemePreset::Teal.palette()), expected);
    }

    #[test]
    fn each_role_sets_exactly_its_slots() {
        let expected: [(ColorRole, &[&str]); 17] = [
            (ColorRole::Background, &["background"]),
            (ColorRole::Panel, &["panel"]),
            (ColorRole::Text, &["text", "fkey_number"]),
            (ColorRole::DimText, &["dim_text", "menu_disabled"]),
            (ColorRole::Directory, &["directory"]),
            (ColorRole::Cursor, &["cursor", "fkey_label"]),
            (
                ColorRole::CursorText,
                &[
                    "cursor_text",
                    "title_text",
                    "fkey_label_text",
                    "menu_shortcut_on_cursor",
                ],
            ),
            (ColorRole::Tagged, &["tagged", "tagged_inactive"]),
            (ColorRole::Border, &["border"]),
            (ColorRole::Bar, &["title"]),
            (
                ColorRole::BarInactive,
                &["title_inactive", "border_inactive"],
            ),
            (ColorRole::StatusBar, &["status_bar"]),
            (ColorRole::Dialog, &["dialog"]),
            (ColorRole::Menu, &["menu"]),
            (ColorRole::Accent, &["accent", "menu_shortcut"]),
            (ColorRole::Success, &["success"]),
            (ColorRole::Error, &["error"]),
        ];
        assert_eq!(expected.len(), ColorRole::ALL.len());
        let chosen = Color::from_rgb8(1, 2, 3);
        for (role, owned) in expected {
            assert!(ColorRole::ALL.contains(&role));
            let mut palette = Palette::default();
            palette.set(role, chosen);
            let before = slots(&Palette::default());
            for (index, (name, color)) in slots(&palette).into_iter().enumerate() {
                let changed = color != before[index].1;
                assert_eq!(changed, owned.contains(&name), "{} -> {name}", role.name());
            }
        }
    }

    #[test]
    fn errors_name_the_exact_line() {
        let cases: &[(&str, &str, usize)] = &[
            ("bad hex", "[theme]\ntext = \"#12345g\"\n", 2),
            ("alpha", "[theme]\n\ntext = \"#11223344\"\n", 3),
            ("no hash", "[theme]\npanel = \"112233\"\n", 2),
            ("color name", "[theme]\npanel = \"red\"\n", 2),
            (
                "unknown role",
                "[theme]\ntext = \"#fff\"\nfoo = \"#fff\"\n",
                3,
            ),
            ("unknown preset", "[theme]\n\npreset = \"x\"\n", 3),
            ("not a string", "[theme]\ntext = 5\n", 2),
        ];
        for (name, text, line) in cases {
            let (found, reason) = error_of(text);
            assert_eq!(found, Some(*line), "{name}: {reason}");
        }
    }

    #[test]
    fn the_reasons_say_what_is_wrong() {
        let cases = [
            (
                "[theme]\ntext = \"#12345\"\n",
                "\"#12345\" is not a color, expected \"#rrggbb\" or \"#rgb\"",
            ),
            ("[theme]\ntext = \"#11223344\"\n", "is not a color"),
            ("[theme]\ntext = \"fff\"\n", "is not a color"),
            ("[theme]\nfoo = \"#fff\"\n", "unknown color role \"foo\""),
            (
                "[theme]\npreset = \"x\"\n",
                "unknown preset \"x\", expected \"cosmic\" or \"default\" or \"classic\" or \"dracula\"",
            ),
        ];
        for (text, expected) in cases {
            let (_, found) = error_of(text);
            assert!(found.contains(expected), "{found}");
        }
    }

    #[test]
    fn short_and_long_hex_agree_and_case_is_free() {
        let short = load_text("[theme]\npanel = \"#a1F\"\n").palette;
        let long = load_text("[theme]\npanel = \"#AA11ff\"\n").palette;
        assert_eq!(short, long);
        assert_eq!(short.panel, Color::from_rgb8(0xAA, 0x11, 0xFF));
    }

    #[test]
    fn a_preset_is_chosen_by_name() {
        let config = load_text("[theme]\npreset = \"classic\"\n");
        assert_eq!(config.palette, ThemePreset::Classic.palette());
    }

    #[test]
    fn a_role_before_the_preset_still_wins() {
        let config = load_text("[theme]\npanel = \"#010203\"\npreset = \"classic\"\n");
        assert_eq!(config.palette.panel, Color::from_rgb8(1, 2, 3));
        let classic = ThemePreset::Classic.palette();
        assert_eq!(config.palette.text, classic.text);
    }

    #[test]
    fn the_inline_form_loads() {
        let config = load_text("theme = { panel = \"#000\" }\n");
        assert_eq!(config.palette.panel, Color::from_rgb8(0, 0, 0));
    }

    #[test]
    fn no_preset_is_flagged_for_contrast() {
        for preset in ThemePreset::ALL {
            assert_eq!(preset.palette().weakest_contrast(), None, "{preset:?}");
            let text = format!("[theme]\npreset = {:?}\n", preset.name());
            assert_eq!(load_text(&text).contrast_warning, None);
        }
    }

    #[test]
    fn title_and_tagged_text_are_readable_on_their_fills_in_cosmic_and_dracula() {
        for preset in [ThemePreset::Cosmic, ThemePreset::Dracula] {
            let p = preset.palette();
            for (name, text, fill) in [
                ("title_text on title", p.title_text, p.title),
                ("tagged_text on tagged", p.tagged_text, p.tagged),
                ("tagged_directory on tagged", p.tagged_directory, p.tagged),
                ("tagged_mark on tagged", p.tagged_mark, p.tagged),
            ] {
                assert!(
                    contrast_ratio(text, fill) >= READABLE_CONTRAST,
                    "{preset:?} {name}"
                );
            }
        }
    }

    #[test]
    fn the_title_bar_text_is_readable_on_its_fills_in_every_preset() {
        for preset in [
            ThemePreset::Teal,
            ThemePreset::Classic,
            ThemePreset::Cosmic,
            ThemePreset::Dracula,
        ] {
            let p = preset.palette();
            for (name, text, fill) in [
                ("title_text on title", p.title_text, p.title),
                ("dim_text on title_inactive", p.dim_text, p.title_inactive),
                ("accent on background", p.accent, p.background),
                ("dim_text on background", p.dim_text, p.background),
            ] {
                assert!(
                    contrast_ratio(text, fill) >= READABLE_CONTRAST,
                    "{preset:?} {name}"
                );
            }
        }
    }

    #[test]
    fn teal_and_classic_keep_their_old_title_and_tagged_text() {
        for preset in [ThemePreset::Teal, ThemePreset::Classic] {
            let p = preset.palette();
            assert_eq!(p.title_text, p.cursor_text, "{preset:?}");
            assert_eq!(p.tagged_text, p.text, "{preset:?}");
            assert_eq!(p.tagged_directory, p.directory, "{preset:?}");
            assert_eq!(p.tagged_mark, p.accent, "{preset:?}");
        }
    }

    #[test]
    fn a_dark_tagged_fill_keeps_the_cosmic_tagged_text_readable() {
        let config = load_text("[open]\n\n[theme]\ntagged = \"#203040\"\n");
        let p = config.palette;
        for (name, color) in [
            ("tagged_text", p.tagged_text),
            ("tagged_directory", p.tagged_directory),
            ("tagged_mark", p.tagged_mark),
        ] {
            let ratio = contrast_ratio(color, p.tagged);
            assert!(ratio >= 3.0, "{name} {ratio}");
        }
    }

    #[test]
    fn a_light_bar_keeps_the_cosmic_title_text_readable() {
        let light_bar = load_text("[open]\n\n[theme]\nbar = \"#e0e0e0\"\n").palette;
        assert!(contrast_ratio(light_bar.title_text, light_bar.title) >= 3.0);
        let black_cursor_text = load_text("[open]\n\n[theme]\ncursor_text = \"#000000\"\n").palette;
        assert!(contrast_ratio(black_cursor_text.title_text, black_cursor_text.title) >= 3.0);
    }

    #[test]
    fn teal_keeps_blind_role_overrides_exactly() {
        let config = load_text(
            "[open]\n\n[theme]\npreset = \"default\"\ntagged = \"#203040\"\nbar = \"#e0e0e0\"\ncursor_text = \"#000000\"\n",
        );
        let p = config.palette;
        assert_eq!(p.tagged_text, p.text);
        assert_eq!(p.tagged_directory, p.directory);
        assert_eq!(p.tagged_mark, p.accent);
        assert_eq!(p.title_text, Color::from_rgb8(0, 0, 0));
    }

    #[test]
    fn a_light_panel_under_the_default_colors_is_reported_but_kept() {
        let config = load_text("[open]\n\n[theme]\npanel = \"#f4f0ff\"\n");
        assert_eq!(config.palette.panel, Color::from_rgb8(0xF4, 0xF0, 0xFF));
        let warning = config.contrast_warning.unwrap();
        assert_eq!(warning.foreground, ColorRole::Directory);
        assert_eq!(warning.background, ColorRole::Panel);
        assert_eq!(warning.line, Some(4));
        assert!(warning.ratio < READABLE_CONTRAST);
    }

    #[test]
    fn a_warning_points_at_the_line_of_the_role_the_user_set() {
        let config =
            load_text("[theme]\nbackground = \"#000\"\npreset = \"classic\"\ntext = \"#0000aa\"\n");
        let warning = config.contrast_warning.unwrap();
        assert_eq!(warning.line, Some(4));
    }
}
