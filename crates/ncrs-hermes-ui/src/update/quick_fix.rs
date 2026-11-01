use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::review_work::{AcceptSummary, ReviewItem};
use ncrs_i18n::Msg;

use super::rules::open_rule_form;
use super::{blocking_overlay_is_open, review_list};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, QuickFixMessage};
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
            actions.push(Action::QuickFixFiled(summary));
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
    let found_access = ctx.filing.and_then(FilingModule::review_access);
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
            file_group(hermes, group, ctx, actions)
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
            actions.push(Action::ExcludeFromHermes(paths));
            Task::none()
        }
    }
}

/// Files the documents of a group one after the other, each through the
/// review's own confirm: never a plain move.
fn file_group(
    hermes: &mut Hermes,
    group: usize,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(menu) = hermes.quick_fix.take() else {
        return Task::none();
    };
    let items = menu.group_items(group);
    let Some(access) = ctx.filing.and_then(FilingModule::review_access) else {
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
    let prefill = rule_prefill_of(item, ctx.default_pattern, ctx.kinds);
    open_rule_form(
        hermes,
        RuleForm::from_document(prefill, ctx.kinds),
        None,
        ctx,
        actions,
    )
}
