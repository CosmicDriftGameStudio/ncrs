//! The rows of a settings page and the sections they sit in: a title, a
//! description and a control on the right, one under the other. Lists of
//! things (folders, rules, tags) use the same rows, with their actions in the
//! row and the "add" button in a row of its own under the list.

use iced::widget::text::Wrapping;
use iced::widget::{container, row, text, Column, Row};
use iced::{alignment, Color, Element, Length, Padding};

use crate::kit::{self, Check, Tone};
use crate::layout::SETTINGS_WIDE_CONTROL_WIDTH;
use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};
use ncrs_i18n::{Language, Msg};

pub struct SettingRow<'a, M> {
    pub label: String,
    /// The Futurama name, from a `Msg`; the only name the command list takes.
    pub theme_name: Option<String>,
    /// The key in the config or theme file, shown where the theme name is but
    /// never offered to the command list.
    pub config_key: Option<String>,
    pub description: Option<String>,
    /// What is wrong with the value, under the description.
    pub problem: Option<String>,
    /// A caution under the description, not an error.
    pub warning: Option<String>,
    pub control: Element<'a, M>,
    /// Rows that show a state or a user's own data are not commands.
    pub in_command_list: bool,
    pub control_width: Option<f32>,
    /// Set for the rows of a list: the control takes the room its buttons
    /// need and the description the rest. The value is how far the row is
    /// set in from the left.
    pub list_indent: Option<f32>,
}

pub struct Group<'a, M> {
    /// A sentence under the title that says what the group is for.
    pub intro: Option<String>,
    pub title: String,
    pub rows: Vec<SettingRow<'a, M>>,
}

impl<M> std::fmt::Debug for SettingRow<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingRow")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl<M> std::fmt::Debug for Group<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Group")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl<'a, M: Clone + 'a> SettingRow<'a, M> {
    /// A row whose texts are worded already. It shows state or the user's own
    /// data, so the command list does not take it.
    pub fn text(label: String, description: Option<String>, control: Element<'a, M>) -> Self {
        Self {
            label,
            theme_name: None,
            config_key: None,
            description,
            problem: None,
            warning: None,
            control,
            control_width: Some(SETTINGS_WIDE_CONTROL_WIDTH),
            in_command_list: false,
            list_indent: None,
        }
    }

    /// A row of a list set in under the row above it, for the members of a
    /// group.
    pub fn indented(self) -> Self {
        Self {
            list_indent: Some(spacing::L),
            ..self
        }
    }

    /// A row of a list: its name, where it is or what it says, and the
    /// actions on it at the right. `status` is dimmed beside the actions.
    pub fn listed(
        label: String,
        description: Option<String>,
        status: Option<String>,
        actions: Vec<Element<'a, M>>,
        palette: &Palette,
    ) -> Self {
        let mut control = Row::new()
            .spacing(spacing::S)
            .align_y(alignment::Vertical::Center);
        if let Some(shown) = status {
            control = control.push(status_text(shown, palette.dim_text));
        }
        let with_actions = actions
            .into_iter()
            .fold(control, |controls, action| controls.push(action));
        Self {
            list_indent: Some(0.0),
            ..Self::text(label, description, with_actions.into())
        }
    }

    /// The row under a list: the buttons that add to it, and `empty` when
    /// the list has nothing yet.
    pub fn adding(
        empty: Option<String>,
        problem: Option<String>,
        buttons: Vec<Element<'a, M>>,
    ) -> Self {
        Self {
            problem,
            list_indent: Some(0.0),
            ..Self::text(
                String::new(),
                empty,
                Row::with_children(buttons)
                    .spacing(spacing::S)
                    .align_y(alignment::Vertical::Center)
                    .into(),
            )
        }
    }
}

/// A row for the command list: `label` and `note` are `Msg`s.
pub fn simple_row<'a, M: Clone + 'a>(
    label: Msg,
    note: Option<Msg>,
    control: Element<'a, M>,
    lang: Language,
) -> SettingRow<'a, M> {
    SettingRow {
        label: lang.text(label).to_owned(),
        theme_name: None,
        config_key: None,
        description: note.map(|msg| lang.text(msg).to_owned()),
        problem: None,
        warning: None,
        control,
        control_width: None,
        in_command_list: true,
        list_indent: None,
    }
}

/// A word in the body size that never wraps.
pub fn status_text<'a, M: Clone + 'a>(content: String, color: Color) -> Element<'a, M> {
    text(content)
        .size(font_size::BODY)
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

/// `value` with a secondary button beside it.
pub fn with_button<'a, M: Clone + 'a>(
    value: Element<'a, M>,
    label: &str,
    on_press: M,
    palette: &Palette,
) -> Element<'a, M> {
    row![
        value,
        kit::button(label, Some(on_press), Tone::Secondary, palette)
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// A tick box with the word "on" or "off" beside it.
pub fn switch<'a, M: Clone + 'a>(
    on: bool,
    on_press: M,
    lang: Language,
    palette: &Palette,
) -> Element<'a, M> {
    let state = if on { Check::On } else { Check::Off };
    let word = lang.text(if on {
        Msg::SettingsOn
    } else {
        Msg::SettingsOff
    });
    row![
        kit::checkbox(state, Some(on_press), palette),
        status_text(word.to_owned(), palette.dim_text),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The sections of a page: each titled group as a heading over its rows, an
/// untitled group as plain rows. `shown` leaves out the rows a search does
/// not find; `marked` is the label of the row the command list asked for.
pub fn sections<'a, M: Clone + 'a>(
    groups: Vec<Group<'a, M>>,
    shown: impl Fn(&SettingRow<'a, M>) -> bool,
    marked: Option<&str>,
    palette: &Palette,
) -> Vec<Element<'a, M>> {
    let mut sections = Vec::new();
    let mut first = true;
    for group in groups {
        let rows: Vec<Element<'a, M>> = group
            .rows
            .into_iter()
            .filter(|setting| shown(setting))
            .map(|setting| {
                let is_marked = marked == Some(setting.label.as_str());
                let lines_below: Vec<Element<'a, M>> = setting
                    .problem
                    .iter()
                    .map(|wrong| kit::error_line(wrong.clone(), palette))
                    .chain(
                        setting
                            .warning
                            .iter()
                            .map(|caution| kit::warning_line(caution.clone(), palette)),
                    )
                    .collect();
                let plain_row = kit::form_row_with_problem(
                    &setting.label,
                    setting
                        .theme_name
                        .as_deref()
                        .or(setting.config_key.as_deref()),
                    setting.description.as_deref(),
                    lines_below,
                    setting.control,
                    if setting.list_indent.is_some() {
                        Length::Shrink
                    } else {
                        Length::Fixed(setting.control_width.unwrap_or(size::CONTROL_WIDTH))
                    },
                    palette,
                );
                let form_row = match setting.list_indent.filter(|indent| *indent > 0.0) {
                    Some(indent) => container(plain_row)
                        .padding(Padding {
                            left: indent,
                            ..Padding::ZERO
                        })
                        .into(),
                    None => plain_row,
                };
                if is_marked {
                    // The outline is drawn over the content, so the row keeps
                    // its text and controls clear of it.
                    container(form_row)
                        .padding([0.0, spacing::XS])
                        .style(theme::marked_setting(palette))
                        .into()
                } else {
                    form_row
                }
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        let mut content = Column::with_children(rows);
        if let Some(intro) = group.intro {
            content = Column::new()
                .push(kit::dim(intro, palette))
                .push(content)
                .spacing(spacing::XS);
        }
        // A group without a title would only repeat its row's label.
        if group.title.is_empty() {
            sections.push(
                container(content)
                    .padding(Padding {
                        top: spacing::M,
                        ..Padding::ZERO
                    })
                    .into(),
            );
            first = false;
            continue;
        }
        sections.push(if first {
            kit::first_section(&group.title, content, palette)
        } else {
            kit::section(&group.title, content, palette)
        });
        first = false;
    }
    sections
}
