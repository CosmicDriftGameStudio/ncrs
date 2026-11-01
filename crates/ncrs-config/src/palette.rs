//! How the `[theme]` section of the config changes the window colors. The
//! colors themselves (`Palette`, `ThemePreset`, `ColorRole`) live in the UI kit.

use std::ops::Range;

pub use ncrs_ui_kit::palette::*;

use crate::config::ThemeSection;
use crate::vscode_theme::VscodeTheme;

/// The key of `[theme]` that picks the base palette.
pub const PRESET_KEY: &str = "preset";
/// Like `preset`, not a role: a VS Code theme file laid over the preset.
pub const VSCODE_KEY: &str = "vscode";

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

/// Builds a palette from the `[theme]` section of the config.
pub trait PaletteFromSection {
    /// The palette `[theme]` describes: the preset (wherever it stands in the
    /// section), then the VS Code theme read from its `vscode` entry, then the
    /// roles in the order written.
    fn from_section(
        section: &ThemeSection,
        vscode: Option<&VscodeTheme>,
    ) -> Result<ConfiguredPalette, ThemeError>;
}

impl PaletteFromSection for Palette {
    fn from_section(
        section: &ThemeSection,
        vscode: Option<&VscodeTheme>,
    ) -> Result<ConfiguredPalette, ThemeError> {
        let mut preset_span = None;
        let mut vscode_span = None;
        let mut preset = ThemePreset::DEFAULT;
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
    use iced::Color;
    use std::path::PathBuf;

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

    #[test]
    fn without_a_theme_section_the_default_preset_applies() {
        for text in ["", "[open]\nview = [\"less\"]\n", "[theme]\n"] {
            let config = load_text(text);
            assert_eq!(config.palette, ThemePreset::Dracula.palette(), "{text:?}");
            assert_eq!(config.contrast_warning, None);
        }
    }

    #[test]
    fn a_config_that_names_cosmic_keeps_the_cosmic_look() {
        let config = load_text("[theme]\npreset = \"cosmic\"\n");
        assert_eq!(config.palette, ThemePreset::Cosmic.palette());
        assert_ne!(ThemePreset::Cosmic.palette(), Palette::default());
    }

    #[test]
    fn the_name_default_picks_the_teal_look() {
        let config = load_text("[theme]\npreset = \"default\"\n");
        assert_eq!(config.palette, ThemePreset::Teal.palette());
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
        let light_bar =
            load_text("[open]\n\n[theme]\nbar = \"#e0e0e0\"\npreset = \"cosmic\"\n").palette;
        assert!(contrast_ratio(light_bar.title_text, light_bar.title) >= 3.0);
        let black_cursor_text =
            load_text("[open]\n\n[theme]\ncursor_text = \"#000000\"\npreset = \"cosmic\"\n")
                .palette;
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
        let config = load_text("[open]\n\n[theme]\npanel = \"#f4f0ff\"\npreset = \"cosmic\"\n");
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
