//! The font of the whole interface: which monospace family `APP_FONT` stands
//! for and how much everything is scaled. The family is set in the font
//! database of iced, so the ~200 places that use `APP_FONT` stay as they are.

use std::borrow::Cow;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{OnceLock, PoisonError, RwLockWriteGuard};

use iced::advanced::graphics::text::cosmic_text::fontdb::{Database, Family, ID};
use iced::advanced::graphics::text::{font_system, FontSystem};

use crate::BUNDLED_FAMILY;

/// The size of the interface text in pixels that a scale of 1.0 stands for.
const BASE_UI_PIXELS: f32 = 13.0;
/// macOS counts a point as a pixel; the other systems draw 96 pixels per 72 points.
const PIXELS_PER_POINT: f32 = if cfg!(target_os = "macos") {
    1.0
} else {
    96.0 / 72.0
};
const MIN_SCALE: f32 = 0.5;
const MAX_SCALE: f32 = 3.0;
/// The monospace family macOS ships and the app has always drawn there.
const MACOS_FAMILY: &str = "Menlo";
/// Fonts of iced itself that are no choice for the user.
const INTERNAL_FAMILIES: [&str; 1] = ["Iced-Icons"];
/// Words at the end of a Pango font name that name the style, not the family.
const STYLE_WORDS: [&str; 5] = ["Regular", "Bold", "Italic", "Oblique", "Book"];

/// What the desktop says about its fonts; read on Linux, where the desktop
/// settings hold them. Other systems have no such answer and keep the sizes
/// the app is built for.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemFont {
    /// The monospace family of the desktop.
    pub family: String,
    /// The size of the desktop's interface font, in points.
    pub points: f32,
    /// The text scaling factor of the desktop, 1.0 when it has none.
    pub scale: f32,
}

impl SystemFont {
    /// The size the user sees as "the system size": points times text scaling.
    pub fn effective_points(&self) -> f32 {
        self.points * self.scale
    }
}

/// The fonts of the desktop, read once: a change of the desktop settings is
/// seen at the next start.
pub fn system_font() -> Option<SystemFont> {
    static SYSTEM: OnceLock<Option<SystemFont>> = OnceLock::new();
    SYSTEM.get_or_init(read_system_font).clone()
}

fn read_system_font() -> Option<SystemFont> {
    if cfg!(any(target_os = "macos", windows)) {
        return None;
    }
    gnome_font().or_else(kde_font)
}

fn gnome_font() -> Option<SystemFont> {
    let (family, mono_points) = parse_pango_font(&gsettings("monospace-font-name")?)?;
    let points = gsettings("font-name")
        .and_then(|text| parse_pango_font(&text))
        .map_or(mono_points, |(_, points)| points);
    let scale = gsettings("text-scaling-factor")
        .and_then(|text| parse_scaling(&text))
        .unwrap_or(1.0);
    Some(SystemFont {
        family,
        points,
        scale,
    })
}

fn gsettings(key: &str) -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", key])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    String::from_utf8(output.stdout).ok()
}

fn kde_font() -> Option<SystemFont> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    let text = std::fs::read_to_string(config.join("kdeglobals")).ok()?;
    let (family, mono_points) = parse_kde_font(&kde_general_value(&text, "fixed")?)?;
    let points = kde_general_value(&text, "font")
        .and_then(|value| parse_kde_font(&value))
        .map_or(mono_points, |(_, points)| points);
    Some(SystemFont {
        family,
        points,
        scale: 1.0,
    })
}

/// `'Ubuntu Mono 13'`, as `gsettings get` and Pango spell a font: the family,
/// then the size in points.
fn parse_pango_font(text: &str) -> Option<(String, f32)> {
    let name = text.trim().trim_matches(['\'', '"']).trim();
    let (family, size) = name.rsplit_once(' ')?;
    let points = positive_number(size)?;
    let mut words: Vec<&str> = family.split_whitespace().collect();
    while words.len() > 1 && words.last().is_some_and(|word| STYLE_WORDS.contains(word)) {
        words.pop();
    }
    (!words.is_empty()).then(|| (words.join(" "), points))
}

/// `Hack,10,-1,5,50,0,0,0,0,0`, the Qt spelling in `kdeglobals`.
fn parse_kde_font(value: &str) -> Option<(String, f32)> {
    let mut parts = value.split(',');
    let family = parts.next()?.trim();
    let points = positive_number(parts.next()?)?;
    (!family.is_empty()).then(|| (family.to_owned(), points))
}

fn parse_scaling(text: &str) -> Option<f32> {
    positive_number(text)
}

fn positive_number(text: &str) -> Option<f32> {
    text.trim()
        .parse::<f32>()
        .ok()
        .filter(|number| number.is_finite() && *number > 0.0)
}

/// The value of `key` in the `[General]` group of a `kdeglobals` text.
fn kde_general_value(text: &str, key: &str) -> Option<String> {
    let mut in_general = false;
    for line in text.lines().map(str::trim) {
        if let Some(group) = line.strip_prefix('[') {
            in_general = group.trim_end_matches(']') == "General";
        } else if in_general {
            if let Some((name, value)) = line.split_once('=') {
                if name.trim() == key {
                    return Some(value.trim().to_owned());
                }
            }
        }
    }
    None
}

fn lock() -> RwLockWriteGuard<'static, FontSystem> {
    font_system()
        .write()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The monospace families the font database holds, sorted and without repeats.
pub fn installed_monospace() -> Vec<String> {
    let mut system = lock();
    let db = system.raw().db();
    let mut names: Vec<String> = db
        .faces()
        .filter(|face| face.monospaced && draws_text(db, face.id))
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .filter(|name| !INTERNAL_FAMILIES.contains(&name.as_str()))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names
}

/// The family `APP_FONT` has without a choice of the user: what the app has
/// always drawn.
pub fn default_family_name() -> &'static str {
    if cfg!(target_os = "macos") {
        MACOS_FAMILY
    } else {
        BUNDLED_FAMILY
    }
}

/// The family shipped inside the app, where the app ships one: macOS keeps
/// its own monospace font.
pub fn bundled_family() -> Option<&'static str> {
    (!cfg!(target_os = "macos")).then_some(BUNDLED_FAMILY)
}

/// The family the "System" entry of the font list stands for.
pub fn system_family_name(system: Option<&SystemFont>) -> String {
    system.map_or_else(
        || default_family_name().to_owned(),
        |font| font.family.clone(),
    )
}

/// The size in points the "System" entry of the size list stands for.
pub fn system_points(system: Option<&SystemFont>) -> f32 {
    system.map_or(
        BASE_UI_PIXELS / PIXELS_PER_POINT,
        SystemFont::effective_points,
    )
}

fn is_installed(system: &mut FontSystem, name: &str) -> bool {
    let db = system.raw().db();
    db.faces()
        .any(|face| face.families.iter().any(|(known, _)| known == name) && draws_text(db, face.id))
}

/// Whether the face has outlines and horizontal metrics. A bitmap-only font
/// such as macOS' "GB18030 Bitmap" has neither, and swash panics drawing it.
fn draws_text(db: &Database, id: ID) -> bool {
    db.with_face_data(id, |data, index| {
        ttf_parser::Face::parse(data, index).is_ok_and(|face| {
            let tables = face.tables();
            tables.hmtx.is_some()
                && (tables.glyf.is_some() || tables.cff.is_some() || tables.cff2.is_some())
        })
    })
    .unwrap_or(false)
}

/// Makes `family` the monospace font of the whole window, `None` the font of
/// the system. A family that is not installed counts as `None`. Text drawn
/// afterwards uses it: iced lays out its paragraphs again when the version of
/// its font system changes, and this bumps it.
pub fn apply_font(family: Option<&str>) {
    let system_font = system_font();
    let mut system = lock();
    static ORIGINAL: OnceLock<String> = OnceLock::new();
    // The family the database starts with, kept so that "System" can put it
    // back: on macOS that name is no installed font, and the fallback of the
    // text system picks the system's monospace font for it.
    let original = ORIGINAL.get_or_init(|| {
        if cfg!(target_os = "macos") {
            system.raw().db().family_name(&Family::Monospace).to_owned()
        } else {
            BUNDLED_FAMILY.to_owned()
        }
    });
    let wanted = match family {
        Some(name) if is_installed(&mut system, name) => name.to_owned(),
        Some(_) | None => match system_font {
            Some(font) if is_installed(&mut system, &font.family) => font.family,
            Some(_) | None => original.clone(),
        },
    };
    if system.raw().db().family_name(&Family::Monospace) == wanted {
        return;
    }
    system.raw().db_mut().set_monospace_family(wanted);
    // An empty font adds no face but counts as a loaded font: the only way
    // to raise the version of the font system, which the paragraphs compare.
    system.load_font(Cow::Owned(Vec::new()));
}

/// The factor for iced's `scale_factor` that makes the interface text
/// `points` large, or the size of the system for `None`. 1.0 is the size the
/// layout is drawn for.
pub fn ui_scale(points: Option<u8>, system: Option<&SystemFont>) -> f32 {
    scale_with(points, system, PIXELS_PER_POINT)
}

fn scale_with(points: Option<u8>, system: Option<&SystemFont>, pixels_per_point: f32) -> f32 {
    let wanted = match (points, system) {
        (Some(set), _) => f32::from(set),
        (None, Some(font)) => font.effective_points(),
        (None, None) => return 1.0,
    };
    (wanted * pixels_per_point / BASE_UI_PIXELS).clamp(MIN_SCALE, MAX_SCALE)
}

// A failing assertion in a test is the signal, so `unwrap` belongs here.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gsettings_font_is_the_family_and_the_size() {
        assert_eq!(
            parse_pango_font("'Ubuntu Mono 13'\n"),
            Some(("Ubuntu Mono".to_owned(), 13.0))
        );
        assert_eq!(
            parse_pango_font("'Cantarell 11.5'"),
            Some(("Cantarell".to_owned(), 11.5))
        );
    }

    #[test]
    fn style_words_after_the_family_are_dropped() {
        assert_eq!(
            parse_pango_font("'DejaVu Sans Mono Bold 11'"),
            Some(("DejaVu Sans Mono".to_owned(), 11.0))
        );
    }

    #[test]
    fn a_font_without_a_size_is_no_font() {
        assert_eq!(parse_pango_font("'Ubuntu Mono'"), None);
        assert_eq!(parse_pango_font("''"), None);
        assert_eq!(parse_pango_font("'Ubuntu Mono 0'"), None);
    }

    #[test]
    fn a_kde_font_is_the_first_two_fields() {
        assert_eq!(
            parse_kde_font("Hack,10,-1,5,50,0,0,0,0,0"),
            Some(("Hack".to_owned(), 10.0))
        );
        assert_eq!(parse_kde_font(",10"), None);
        assert_eq!(parse_kde_font("Hack"), None);
    }

    #[test]
    fn only_the_general_group_of_kdeglobals_counts() {
        let text = "[KDE]\nfixed=Wrong,9\n[General]\nfixed=Hack,10,-1\nfont=Noto Sans,11,-1\n";
        assert_eq!(
            kde_general_value(text, "fixed").as_deref(),
            Some("Hack,10,-1")
        );
        assert_eq!(
            kde_general_value(text, "font").as_deref(),
            Some("Noto Sans,11,-1")
        );
        assert_eq!(kde_general_value(text, "menuFont"), None);
    }

    #[test]
    fn a_scaling_factor_must_be_a_positive_number() {
        assert_eq!(parse_scaling("1.25\n"), Some(1.25));
        assert_eq!(parse_scaling("0.0"), None);
        assert_eq!(parse_scaling("uint32 1"), None);
    }

    #[test]
    fn the_system_size_times_its_scaling_decides_the_scale() {
        let desktop = SystemFont {
            family: "Hack".to_owned(),
            points: 10.0,
            scale: 1.5,
        };
        let expected = 10.0 * 1.5 * 96.0 / 72.0 / BASE_UI_PIXELS;
        assert!((scale_with(None, Some(&desktop), 96.0 / 72.0) - expected).abs() < 1e-6);
        assert_eq!(scale_with(None, None, 96.0 / 72.0), 1.0);
    }

    #[test]
    fn a_chosen_size_is_points_to_pixels_over_the_base() {
        assert_eq!(scale_with(Some(13), None, 1.0), 1.0);
        assert_eq!(scale_with(Some(26), None, 1.0), 2.0);
        let on_linux = scale_with(Some(12), None, 96.0 / 72.0);
        assert!((on_linux - 16.0 / BASE_UI_PIXELS).abs() < 1e-6);
    }

    #[test]
    fn the_scale_stays_in_a_usable_range() {
        let huge = SystemFont {
            family: String::new(),
            points: 40.0,
            scale: 3.0,
        };
        assert_eq!(scale_with(None, Some(&huge), 1.0), MAX_SCALE);
        assert_eq!(scale_with(Some(1), None, 1.0), MIN_SCALE);
    }

    fn database_with(font: Vec<u8>) -> (Database, ID) {
        let mut db = Database::new();
        db.load_font_data(font);
        let id = db.faces().next().map(|face| face.id).unwrap();
        (db, id)
    }

    #[test]
    fn a_font_without_horizontal_metrics_draws_no_text() {
        let (outline_db, outline_id) = database_with(crate::BUNDLED_REGULAR.to_vec());
        assert!(draws_text(&outline_db, outline_id));

        // Renaming the tag in the table directory hides the table, as in a
        // bitmap-only font.
        let mut font = crate::BUNDLED_REGULAR.to_vec();
        let hmtx = font[..512]
            .windows(4)
            .position(|tag| tag == b"hmtx")
            .unwrap();
        font[hmtx..hmtx + 4].copy_from_slice(b"xxxx");
        let (bitmap_db, bitmap_id) = database_with(font);
        assert!(!draws_text(&bitmap_db, bitmap_id));
    }
}
