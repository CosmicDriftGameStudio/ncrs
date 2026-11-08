//! The Lucide icons the main window draws.

use iced::widget::svg::{self, Svg};
use iced::{Color, Element, Radians, Rotation, Theme};
use std::sync::LazyLock;

use crate::theme::size;

/// The Lucide icons the main window draws (assets/icons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Clock,
    Loader,
    CircleCheck,
    CircleHelp,
    TriangleAlert,
    Ban,
    ChevronRight,
    CircleArrowRight,
}

impl Icon {
    pub const ALL: [Self; 8] = [
        Self::Clock,
        Self::Loader,
        Self::CircleCheck,
        Self::CircleHelp,
        Self::TriangleAlert,
        Self::Ban,
        Self::ChevronRight,
        Self::CircleArrowRight,
    ];

    pub(super) const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Clock => include_bytes!("../../../../assets/icons/clock.svg"),
            Self::Loader => include_bytes!("../../../../assets/icons/loader-circle.svg"),
            Self::CircleCheck => include_bytes!("../../../../assets/icons/circle-check.svg"),
            Self::CircleHelp => include_bytes!("../../../../assets/icons/circle-question-mark.svg"),
            Self::TriangleAlert => include_bytes!("../../../../assets/icons/triangle-alert.svg"),
            Self::Ban => include_bytes!("../../../../assets/icons/ban.svg"),
            Self::ChevronRight => include_bytes!("../../../../assets/icons/chevron-right.svg"),
            Self::CircleArrowRight => {
                include_bytes!("../../../../assets/icons/circle-arrow-right.svg")
            }
        }
    }

    /// Built once: the handle's id is what the renderer caches the parsed
    /// vector by, so a new handle per frame would parse the file every frame.
    pub(super) fn handle(self) -> svg::Handle {
        static HANDLES: LazyLock<[svg::Handle; 8]> =
            LazyLock::new(|| Icon::ALL.map(|icon| svg::Handle::from_memory(icon.bytes())));
        HANDLES
            .get(self as usize)
            .cloned()
            .unwrap_or_else(|| svg::Handle::from_memory(self.bytes()))
    }
}

/// An icon in `color`, [`size::ICON`] square.
pub fn icon<'a, M: 'a>(icon: Icon, color: Color) -> Element<'a, M> {
    icon_rotated(icon, color, 0.0)
}

/// As [`icon`], turned by `radians` around its center (the spinner).
pub fn icon_rotated<'a, M: 'a>(icon: Icon, color: Color, radians: f32) -> Element<'a, M> {
    Svg::new(icon.handle())
        .width(size::ICON)
        .height(size::ICON)
        .rotation(Rotation::Floating(Radians(radians)))
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_a_24_unit_svg() {
        for icon in Icon::ALL {
            let source = std::str::from_utf8(icon.bytes()).unwrap_or_default();
            assert!(source.trim_start().starts_with("<svg"), "{icon:?}");
            assert!(source.contains("viewBox=\"0 0 24 24\""), "{icon:?}");
        }
    }

    #[test]
    fn every_icon_has_its_own_cached_handle() {
        for (index, icon) in Icon::ALL.into_iter().enumerate() {
            assert_eq!(icon as usize, index);
            assert_eq!(icon.handle().id(), icon.handle().id());
        }
    }
}
