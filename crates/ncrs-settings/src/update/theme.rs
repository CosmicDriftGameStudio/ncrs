use iced::widget::operation;
use iced::Task;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::focus::release_focus;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, ThemeFetchMessage};
use crate::open_vsx::{ExtensionId, FetchError, ThemeFileUrl, ThemeSource};
use crate::state::Settings;
use crate::theme_fetch::install::{download_theme, install_theme_from_link};
use crate::theme_fetch::view::FIELD_ID;
use crate::theme_fetch::{Fetched, Installed, Pick, Stage, ThemeFetch};

pub fn update(
    settings: &mut Settings,
    message: ThemeFetchMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        ThemeFetchMessage::Open => open(settings, ctx, actions),
        ThemeFetchMessage::Close => close(settings),
        ThemeFetchMessage::TextChanged(text) => {
            if let Some(card) = settings.theme_fetch.as_mut() {
                card.set_text(text);
            }
            Task::none()
        }
        ThemeFetchMessage::Activate => activate(settings, ctx),
        ThemeFetchMessage::Move(delta) => {
            if let Some(card) = settings.theme_fetch.as_mut() {
                card.move_cursor(delta);
            }
            Task::none()
        }
        ThemeFetchMessage::Pick(index) => {
            if let Some(card) = settings.theme_fetch.as_mut() {
                card.select(index);
            }
            activate(settings, ctx)
        }
        ThemeFetchMessage::Fetched { generation, result } => {
            fetched(settings, generation, result, ctx)
        }
        ThemeFetchMessage::Installed { generation, result } => {
            installed(settings, generation, result, ctx, actions)
        }
    }
}

fn open(settings: &mut Settings, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    let focus = operation::focus(FIELD_ID);
    if settings.theme_fetch.is_some() {
        return focus;
    }
    if ctx.blocking_overlay_open {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    settings.theme_fetch_generation = settings.theme_fetch_generation.wrapping_add(1);
    settings.theme_fetch = Some(ThemeFetch::new());
    focus
}

/// Not while the theme is written: the config change would have no card
/// left to report to.
fn close(settings: &mut Settings) -> Task<Message> {
    if settings
        .theme_fetch
        .as_ref()
        .is_some_and(|card| matches!(card.stage(), Stage::Installing))
    {
        return Task::none();
    }
    if settings.theme_fetch.take().is_some() {
        release_focus()
    } else {
        Task::none()
    }
}

/// Enter or a click: fetches what was typed, or installs the chosen theme.
fn activate(settings: &mut Settings, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(card) = settings.theme_fetch.as_mut() else {
        return Task::none();
    };
    match card.stage() {
        Stage::Input => match card.start() {
            Some(ThemeSource::Extension(id)) => start_download(settings, id, ctx),
            Some(ThemeSource::File(link)) => start_link_install(settings, link, ctx),
            None => Task::none(),
        },
        Stage::Choosing { .. } => match card.choose() {
            Some(pick) => start_install(settings, pick, ctx),
            None => Task::none(),
        },
        Stage::Fetching | Stage::Installing => Task::none(),
    }
}

fn start_download(settings: &Settings, id: ExtensionId, ctx: &Ctx<'_>) -> Task<Message> {
    let generation = settings.theme_fetch_generation;
    let fetch = ctx.services.fetch_extension;
    Task::perform(
        tokio::task::spawn_blocking(move || download_theme(fetch, &id)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(FetchError::Network(join.to_string())));
            Message::ThemeFetch(ThemeFetchMessage::Fetched { generation, result })
        },
    )
}

fn start_link_install(settings: &mut Settings, link: ThemeFileUrl, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(config_path) = ctx.config_path.map(std::path::Path::to_path_buf) else {
        return fail(settings, FetchError::NoConfigPath);
    };
    let generation = settings.theme_fetch_generation;
    let failed_path = config_path.display().to_string();
    Task::perform(
        tokio::task::spawn_blocking(move || install_theme_from_link(&link, &config_path)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| {
                Err(FetchError::WriteFailed {
                    path: failed_path,
                    reason: join.to_string(),
                })
            });
            Message::ThemeFetch(ThemeFetchMessage::Installed { generation, result })
        },
    )
}

fn start_install(settings: &mut Settings, pick: Pick, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(config_path) = ctx.config_path.map(std::path::Path::to_path_buf) else {
        return fail(settings, FetchError::NoConfigPath);
    };
    let generation = settings.theme_fetch_generation;
    let install = ctx.services.install_theme;
    let failed_path = config_path.display().to_string();
    Task::perform(
        tokio::task::spawn_blocking(move || install(&pick, &config_path)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| {
                Err(FetchError::WriteFailed {
                    path: failed_path,
                    reason: join.to_string(),
                })
            });
            Message::ThemeFetch(ThemeFetchMessage::Installed { generation, result })
        },
    )
}

/// Back to the text field with the reason shown.
fn fail(settings: &mut Settings, error: FetchError) -> Task<Message> {
    let Some(card) = settings.theme_fetch.as_mut() else {
        return Task::none();
    };
    card.fail(error);
    operation::focus(FIELD_ID)
}

fn fetched(
    settings: &mut Settings,
    generation: u64,
    result: Result<Fetched, FetchError>,
    ctx: &Ctx<'_>,
) -> Task<Message> {
    if generation != settings.theme_fetch_generation {
        return Task::none();
    }
    let Some(card) = settings.theme_fetch.as_mut() else {
        return Task::none();
    };
    if !matches!(card.stage(), Stage::Fetching) {
        return Task::none();
    }
    match card.finish_fetching(result) {
        Some(pick) => start_install(settings, pick, ctx),
        None if matches!(card.stage(), Stage::Input) => operation::focus(FIELD_ID),
        None => Task::none(),
    }
}

fn installed(
    settings: &mut Settings,
    generation: u64,
    result: Result<Box<Installed>, FetchError>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if generation != settings.theme_fetch_generation || settings.theme_fetch.is_none() {
        return Task::none();
    }
    let installed = match result {
        Ok(installed) => *installed,
        Err(error) => return fail(settings, error),
    };
    settings.theme_fetch = None;
    let Installed {
        config,
        label,
        license,
    } = installed;
    if let Some(state) = settings.page.as_mut() {
        state.file = Ok(Box::new(config.clone()));
    }
    actions.push(Action::ApplyLook(Box::new(config)));
    let shown_license =
        license.unwrap_or_else(|| ctx.lang.text(Msg::ThemeFetchLicenseNotStated).to_owned());
    let notice = fill_template(
        ctx.lang.text(Msg::ThemeFetchInstalled),
        &[
            ("{label}", label.as_str()),
            ("{license}", shown_license.as_str()),
        ],
    );
    actions.push(Action::ShowNotice(notice));
    release_focus()
}
