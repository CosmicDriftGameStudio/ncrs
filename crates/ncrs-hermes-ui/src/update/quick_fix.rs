use std::path::PathBuf;

use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::review_work::{AcceptSummary, ReviewItem};
use ncrs_i18n::{fill_template, Msg};

use super::rules::open_rule_form;
use super::{blocking_overlay_is_open, review_list};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{HermesColumnMessage, Message, QuickFixMessage};
use crate::quick_fix::{field_to_fix, QuickFix, QuickFixAction};
use crate::review_list::rule_prefill_of;
use crate::rule_form::RuleForm;
use crate::state::Hermes;

pub fn update(
    hermes: &mut Hermes,
    message: QuickFixMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        QuickFixMessage::Open => open(hermes, ctx, actions),
        QuickFixMessage::Close => {
            hermes.quick_fix = None;
            Task::none()
        }
        QuickFixMessage::Loaded { generation, result } => {
            loaded(hermes, generation, result, ctx, actions)
        }
        QuickFixMessage::Move(delta) => with_quick_fix(hermes, |menu| menu.move_by(delta)),
        QuickFixMessage::First => with_quick_fix(hermes, QuickFix::select_first),
        QuickFixMessage::Last => with_quick_fix(hermes, QuickFix::select_last),
        QuickFixMessage::Hover(index) => with_quick_fix(hermes, |menu| menu.select(index)),
        QuickFixMessage::Activate => run_entry(hermes, ctx, actions),
        QuickFixMessage::Click(index) => {
            drop(with_quick_fix(hermes, |menu| menu.select(index)));
            run_entry(hermes, ctx, actions)
        }
        QuickFixMessage::Filed(summary) => {
            filed(hermes, &summary, ctx, actions);
            Task::none()
        }
    }
}

fn with_quick_fix(hermes: &mut Hermes, change: impl FnOnce(&mut QuickFix)) -> Task<Message> {
    if let Some(menu) = hermes.quick_fix.as_mut() {
        change(menu);
    }
    Task::none()
}

/// Cmd+.: opens the menu on the inquiries of the active panel, and looks up
/// their reviews on a blocking thread. Says so when there are none.
fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if blocking_overlay_is_open(hermes, ctx)
        || ctx.context_menu_open
        || ctx.search_open
        || !ctx.hermes_active
    {
        return Task::none();
    }
    let paths = ctx.inquiry_paths.clone();
    let found_access = hermes.filing.as_ref().and_then(FilingModule::review_access);
    let Some(access) = found_access.filter(|_| !paths.is_empty()) else {
        let text = ctx.lang.text(Msg::StatusQuickFixNone).to_owned();
        actions.push(Action::ShowNotice(text));
        return Task::none();
    };
    actions.push(Action::ClearStatus);
    hermes.quick_fix_generation = hermes.quick_fix_generation.wrapping_add(1);
    let generation = hermes.quick_fix_generation;
    hermes.quick_fix =
        Some(QuickFix::new(ctx.quick_fix_anchor, ctx.window_size).with_kinds(ctx.kinds.clone()));
    Task::perform(
        tokio::task::spawn_blocking(move || access.items_for(&paths)),
        move |joined| {
            let result = joined.unwrap_or_else(|err| Err(err.to_string()));
            Message::QuickFix(QuickFixMessage::Loaded { generation, result })
        },
    )
}

fn loaded(
    hermes: &mut Hermes,
    generation: u64,
    result: Result<Vec<ReviewItem>, String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if generation != hermes.quick_fix_generation || hermes.quick_fix.is_none() {
        return Task::none();
    }
    match result {
        Ok(items) if items.is_empty() => {
            hermes.quick_fix = None;
            let text = ctx.lang.text(Msg::StatusQuickFixNone).to_owned();
            actions.push(Action::ShowNotice(text));
            Task::none()
        }
        Ok(items) => with_quick_fix(hermes, |menu| menu.loaded(items)),
        Err(detail) => {
            hermes.quick_fix = None;
            actions.push(Action::ShowError(detail));
            Task::none()
        }
    }
}

/// Carries out the highlighted entry. The menu closes first for everything
/// that opens or files something else.
fn run_entry(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    let Some(menu) = hermes.quick_fix.as_mut() else {
        return Task::none();
    };
    let Some(action) = menu.selected_action() else {
        return Task::none();
    };
    match action {
        QuickFixAction::Accept(group) if menu.group_items(group).len() > 1 => {
            menu.summarise(group);
            Task::none()
        }
        QuickFixAction::Accept(group) | QuickFixAction::FileGroup(group) => {
            file_group(hermes, group, actions)
        }
        QuickFixAction::Back => {
            menu.back();
            Task::none()
        }
        QuickFixAction::SetFields => open_review_from_menu(hermes, ctx, actions),
        QuickFixAction::CreateRule => open_rule_form_from_menu(hermes, ctx, actions),
        QuickFixAction::Later => {
            hermes.quick_fix = None;
            Task::none()
        }
        QuickFixAction::Exclude => {
            let paths = menu.paths();
            hermes.quick_fix = None;
            exclude_from_hermes(hermes, paths, actions)
        }
    }
}

/// Tells the index that Hermes leaves `paths` alone from now on; the column
/// shows them as excluded once the index has answered.
pub fn exclude_from_hermes(
    hermes: &Hermes,
    paths: Vec<PathBuf>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::review_access) else {
        actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
        return Task::none();
    };
    let excluded = paths.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || access.exclude(&paths)),
        move |joined| {
            Message::HermesColumn(HermesColumnMessage::Excluded {
                paths: excluded.clone(),
                result: joined.unwrap_or_else(|err| Err(err.to_string())),
            })
        },
    )
}

/// Files the documents of a group one after the other, each through the
/// review's own confirm: never a plain move.
fn file_group(hermes: &mut Hermes, group: usize, actions: &mut Vec<Action>) -> Task<Message> {
    let Some(menu) = hermes.quick_fix.take() else {
        return Task::none();
    };
    let items = menu.group_items(group);
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::review_access) else {
        actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.accept(&items)),
        |joined| {
            Message::QuickFix(QuickFixMessage::Filed(joined.unwrap_or_else(|err| {
                AcceptSummary {
                    failed: 1,
                    first_failure: Some(err.to_string()),
                    ..AcceptSummary::default()
                }
            })))
        },
    )
}

/// Opens the review page on the document of the menu with its form open, to
/// fill in the missing field.
fn open_review_from_menu(
    hermes: &mut Hermes,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(menu) = hermes.quick_fix.take() else {
        return Task::none();
    };
    let Some(item) = menu.item_to_fix() else {
        return Task::none();
    };
    let field = field_to_fix(item);
    let path = item.path.clone();
    let opened = review_list::open(hermes, ctx, actions);
    if let Some(view) = hermes.review_list.as_mut() {
        view.edit_on_load(path, field);
    }
    opened
}

/// Opens the rule form with what the review would file the document of the
/// menu with.
fn open_rule_form_from_menu(
    hermes: &mut Hermes,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(menu) = hermes.quick_fix.take() else {
        return Task::none();
    };
    let Some(item) = menu.item_to_fix() else {
        return Task::none();
    };
    let prefill = rule_prefill_of(
        item,
        hermes.filing_config.default_pattern.as_deref(),
        ctx.kinds,
    );
    open_rule_form(
        hermes,
        RuleForm::from_document(prefill, ctx.kinds),
        None,
        ctx,
        actions,
    )
}

/// What the status bar says about the documents filed from the menu; `None`
/// when the only one failed, which is an error rather than a notice.
fn accept_notice(summary: &AcceptSummary, lang: ncrs_i18n::Language) -> Option<String> {
    let template = |msg: Msg| lang.text(msg).to_owned();
    match (summary.total(), summary) {
        (
            1,
            AcceptSummary {
                last_filed: Some(path),
                ..
            },
        ) => Some(fill_template(
            lang.text(Msg::StatusQuickFixFiled),
            &[("{path}", &path.display().to_string())],
        )),
        (
            1,
            AcceptSummary {
                target_exists: 1, ..
            },
        ) => Some(template(Msg::StatusQuickFixTargetExists)),
        (1, AcceptSummary { requeued: 1, .. }) => Some(template(Msg::StatusQuickFixRequeued)),
        (1, _) => None,
        _ => Some(fill_template(
            lang.text(Msg::StatusQuickFixSummary),
            &[
                ("{filed}", &summary.filed.to_string()),
                ("{taken}", &summary.target_exists.to_string()),
                ("{requeued}", &summary.requeued.to_string()),
                ("{failed}", &summary.failed.to_string()),
            ],
        )),
    }
}

/// The documents of a group were filed from the menu: the column asks again
/// and the panels read their folders.
fn filed(hermes: &mut Hermes, summary: &AcceptSummary, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    hermes.hermes_column.invalidate_all();
    hermes.hermes_reads.clear();
    hermes.hermes_detail_reads.clear();
    match accept_notice(summary, ctx.lang) {
        Some(text) => actions.push(Action::ShowNotice(text)),
        None => actions.push(Action::ShowError(
            summary.first_failure.clone().unwrap_or_default(),
        )),
    }
    actions.push(Action::ReloadPanels);
}
