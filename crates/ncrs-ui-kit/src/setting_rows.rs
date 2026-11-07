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

/// What the keyboard can do on a row; a row without one is skipped by it.
#[derive(Debug, Clone, PartialEq)]
pub enum RowKey<M> {
    /// Enter and Space press the row's button, switch or opener.
    Press(M),
    /// A choice among a few values: Left and Right change it, Enter and
    /// Space step on to the next one.
    Choice { previous: M, next: M },
    /// A text field: Enter and Space focus it.
    Text(&'static str),
    /// A dropdown: Enter and Space open its list, Up and Down choose in it,
    /// Enter takes the highlighted row and Escape closes it; Left and Right
    /// change the value without opening it. `choices` is what picking each
    /// of `labels` sends.
    Dropdown {
        labels: Vec<String>,
        choices: Vec<M>,
        selected: usize,
    },
    /// A row with several buttons: Left and Right choose one, Enter and
    /// Space press it. Left on the first goes back to the list of pages.
    Buttons(Vec<M>),
}

impl<M> RowKey<M> {
    /// The same key for the messages of the page around.
    pub fn map<N>(self, wrap: impl Fn(M) -> N) -> RowKey<N> {
        match self {
            Self::Press(message) => RowKey::Press(wrap(message)),
            Self::Choice { previous, next } => RowKey::Choice {
                previous: wrap(previous),
                next: wrap(next),
            },
            Self::Text(id) => RowKey::Text(id),
            Self::Buttons(messages) => RowKey::Buttons(messages.into_iter().map(wrap).collect()),
            Self::Dropdown {
                labels,
                choices,
                selected,
            } => RowKey::Dropdown {
                labels,
                choices: choices.into_iter().map(wrap).collect(),
                selected,
            },
        }
    }
}

/// Where the keyboard is on a page: the number of the row among those that
/// have a key, and the highlighted row of its list while that is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowFocus {
    pub row: usize,
    pub open: Option<usize>,
    /// The button of a row with several that Enter presses.
    pub button: usize,
}

/// A button or tick box of a row; one without a message is off.
#[derive(Debug)]
pub struct RowButton<M> {
    pub label: String,
    pub message: Option<M>,
    pub tone: Tone,
    /// A tick box in this state instead of a button.
    pub check: Option<Check>,
}

impl<M> RowButton<M> {
    pub fn new(label: impl Into<String>, message: Option<M>, tone: Tone) -> Self {
        Self {
            label: label.into(),
            message,
            tone,
            check: None,
        }
    }

    pub fn check(state: Check, message: Option<M>) -> Self {
        Self {
            check: Some(state),
            ..Self::new("", message, Tone::Secondary)
        }
    }

    fn reachable(&self) -> bool {
        self.message.is_some()
    }
}

/// What the keyboard presses, one message per reachable button.
pub fn pressable<M: Clone>(buttons: &[RowButton<M>]) -> Vec<M> {
    buttons
        .iter()
        .filter(|button| button.reachable())
        .filter_map(|button| button.message.clone())
        .collect()
}

/// `leading` and the buttons side by side; the `marked`th of those that are
/// on is highlighted.
pub fn button_row<'a, M: Clone + 'a>(
    leading: Option<Element<'a, M>>,
    buttons: &[RowButton<M>],
    marked: Option<usize>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut on = 0;
    let mut strip = Row::new()
        .spacing(spacing::S)
        .align_y(alignment::Vertical::Center);
    if let Some(lead) = leading {
        strip = strip.push(lead);
    }
    for button in buttons {
        let is_marked = button.reachable() && marked == Some(on);
        on += usize::from(button.reachable());
        let tone = match (marked, is_marked, button.tone) {
            (_, true, _) => Tone::Primary,
            // The mark is the only Primary button while the keyboard is here.
            (Some(_), false, Tone::Primary) => Tone::Secondary,
            _ => button.tone,
        };
        strip = strip.push(match button.check {
            Some(state) => {
                let tick = kit::checkbox(state, button.message.clone(), palette);
                if is_marked {
                    container(tick)
                        .padding(spacing::XS)
                        .style(theme::focused_setting(palette))
                        .into()
                } else {
                    tick
                }
            }
            None => kit::button(&button.label, button.message.clone(), tone, palette),
        });
    }
    strip.into()
}

pub struct SettingRow<'a, M> {
    /// Set by the page that builds the row; see [`RowKey`].
    pub key: Option<RowKey<M>>,
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
    /// Set with [`RowKey::Buttons`]: the control once for each button, with
    /// that button marked.
    pub marked_controls: Vec<Element<'a, M>>,
}

pub struct Group<'a, M> {
    /// A sentence under the title that says what the group is for.
    pub intro: Option<String>,
    pub title: String,
    pub rows: Vec<SettingRow<'a, M>>,
}

impl<'a, M: Clone + 'a> Group<'a, M> {
    /// The group for the messages of the page around.
    pub fn map<N: Clone + 'a>(self, wrap: impl Fn(M) -> N + Clone + 'a) -> Group<'a, N> {
        Group {
            intro: self.intro,
            title: self.title,
            rows: self
                .rows
                .into_iter()
                .map(|row| row.map(wrap.clone()))
                .collect(),
        }
    }
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
    /// The row for the messages of the page around.
    pub fn map<N: Clone + 'a>(self, wrap: impl Fn(M) -> N + Clone + 'a) -> SettingRow<'a, N> {
        SettingRow {
            key: self.key.map(|key| key.map(&wrap)),
            label: self.label,
            theme_name: self.theme_name,
            config_key: self.config_key,
            description: self.description,
            problem: self.problem,
            warning: self.warning,
            control: self.control.map(wrap.clone()),
            in_command_list: self.in_command_list,
            control_width: self.control_width,
            list_indent: self.list_indent,
            marked_controls: self
                .marked_controls
                .into_iter()
                .map(|control| control.map(wrap.clone()))
                .collect(),
        }
    }

    /// A row whose texts are worded already. It shows state or the user's own
    /// data, so the command list does not take it.
    pub fn text(label: String, description: Option<String>, control: Element<'a, M>) -> Self {
        Self {
            key: None,
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
            marked_controls: Vec::new(),
        }
    }

    /// A row of a list whose actions the keyboard reaches one by one. A row
    /// with no button on gets no key.
    pub fn listed_buttons(
        label: String,
        description: Option<String>,
        status: Option<String>,
        buttons: Vec<RowButton<M>>,
        palette: &Palette,
    ) -> Self {
        let build = |marked| {
            let leading = status
                .clone()
                .map(|shown| status_text(shown, palette.dim_text));
            button_row(leading, &buttons, marked, palette)
        };
        Self {
            list_indent: Some(0.0),
            ..Self::text(label, description, build(None))
        }
        .with_buttons(pressable(&buttons), build)
    }

    /// The row under a list, with its buttons reachable by keyboard.
    pub fn adding_buttons(
        empty: Option<String>,
        problem: Option<String>,
        buttons: Vec<RowButton<M>>,
        palette: &Palette,
    ) -> Self {
        let build = |marked| button_row(None, &buttons, marked, palette);
        Self {
            problem,
            list_indent: Some(0.0),
            ..Self::text(String::new(), empty, build(None))
        }
        .with_buttons(pressable(&buttons), build)
    }

    /// The keys of `messages` go with the control `build` draws; `build`
    /// marks the button the keyboard is on.
    pub fn with_buttons(
        self,
        messages: Vec<M>,
        build: impl Fn(Option<usize>) -> Element<'a, M>,
    ) -> Self {
        if messages.is_empty() {
            return self;
        }
        Self {
            control: build(None),
            marked_controls: (0..messages.len()).map(|at| build(Some(at))).collect(),
            key: Some(RowKey::Buttons(messages)),
            ..self
        }
    }

    /// The row answers the keyboard as `key` says.
    pub fn keyed(self, key: RowKey<M>) -> Self {
        Self {
            key: Some(key),
            ..self
        }
    }

    /// Like [`Self::keyed`], for a key that may not exist.
    pub fn keyed_maybe(self, key: Option<RowKey<M>>) -> Self {
        match key {
            Some(row_key) => self.keyed(row_key),
            None => self,
        }
    }

    /// Like [`Self::keyed`], only while `when` holds: a row whose buttons are
    /// off answers no key either.
    pub fn keyed_if(self, when: bool, key: RowKey<M>) -> Self {
        if when {
            self.keyed(key)
        } else {
            self
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
        key: None,
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
        marked_controls: Vec::new(),
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
    sections_with_focus(groups, shown, marked, None, |_, _| None, palette)
}

/// The keys of the rows [`sections_with_focus`] shows, in order: the numbers
/// `focused` and `on_focus` count in.
pub fn keyed_rows<'a, M: Clone + 'a>(
    groups: Vec<Group<'a, M>>,
    shown: impl Fn(&SettingRow<'a, M>) -> bool,
) -> Vec<RowKey<M>> {
    groups
        .into_iter()
        .flat_map(|group| group.rows)
        .filter(|setting| shown(setting))
        .filter_map(|setting| setting.key)
        .collect()
}

/// Like [`sections`], with the keyboard focus on the row `focus` names. A
/// click on a row, or on its control, sends what `on_focus` makes of the
/// row's number: the control's own message goes along as `Some`, a click on
/// the row beside it as `None`; `None` back sends nothing.
pub fn sections_with_focus<'a, M: Clone + 'a>(
    groups: Vec<Group<'a, M>>,
    shown: impl Fn(&SettingRow<'a, M>) -> bool,
    marked: Option<&str>,
    focus: Option<RowFocus>,
    on_focus: impl Fn(usize, Option<M>) -> Option<M> + Copy + 'a,
    palette: &Palette,
) -> Vec<Element<'a, M>> {
    let focused = focus.map(|at| at.row);
    let mut sections = Vec::new();
    let mut first = true;
    let mut keyed_so_far = 0;
    for group in groups {
        let rows: Vec<Element<'a, M>> = group
            .rows
            .into_iter()
            .filter(|setting| shown(setting))
            .map(|setting| {
                let number = setting.key.as_ref().map(|_| {
                    keyed_so_far += 1;
                    keyed_so_far - 1
                });
                let is_marked = marked == Some(setting.label.as_str());
                let has_focus = number.is_some() && number == focused;
                let mut marked_controls = setting.marked_controls;
                let marked_control = match focus {
                    Some(at) if has_focus && !marked_controls.is_empty() => {
                        let last = marked_controls.len() - 1;
                        marked_controls.swap_remove(at.button.min(last))
                    }
                    _ => setting.control,
                };
                let opened_control = match (&setting.key, focus) {
                    (
                        Some(RowKey::Dropdown {
                            labels,
                            choices,
                            selected,
                        }),
                        Some(RowFocus {
                            open: Some(highlighted),
                            ..
                        }),
                    ) if has_focus => crate::open_list::open_list(
                        labels,
                        choices,
                        highlighted,
                        *selected,
                        setting.control_width.unwrap_or(size::CONTROL_WIDTH),
                        palette,
                    ),
                    _ => marked_control,
                };
                let row_control = match number {
                    Some(at) => opened_control
                        .map(move |message| on_focus(at, Some(message.clone())).unwrap_or(message)),
                    None => opened_control,
                };
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
                    row_control,
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
                let row: Element<'a, M> = if has_focus {
                    // Like the mark: drawn over the content, which keeps clear.
                    container(form_row)
                        .id(crate::row_scroll::FOCUSED_ROW_ID)
                        .padding([0.0, spacing::XS])
                        .style(theme::focused_setting(palette))
                        .into()
                } else if is_marked {
                    // The outline is drawn over the content, so the row keeps
                    // its text and controls clear of it.
                    container(form_row)
                        .padding([0.0, spacing::XS])
                        .style(theme::marked_setting(palette))
                        .into()
                } else {
                    form_row
                };
                match number.and_then(|at| on_focus(at, None)) {
                    Some(message) => iced::widget::mouse_area(row).on_press(message).into(),
                    None => row,
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
