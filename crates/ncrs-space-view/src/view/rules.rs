//! The page that says what counts as safe, how things are deleted and what is never touched.

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length, Padding};
use ncrs_i18n::{fill_template, Msg};
use ncrs_space::{catalog_msgs, CatalogId, Group};
use ncrs_ui_kit::color_chip::color_chip;
use ncrs_ui_kit::info_card::{card_note, card_text, dot_line, info_card, key_row};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::page_scroll::page_scroll_padded;
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, spacing};

use super::list::group_color;
use crate::ctx::ViewCtx;
use crate::message::Message;

const RULES_SCROLL_ID: &str = "space-rules";

const OPEN_TAB_KEY: &str = if cfg!(target_os = "macos") {
    "\u{2318}\u{2325}P"
} else {
    "Ctrl+Alt+P"
};

fn catalog_name(id: CatalogId, ctx: ViewCtx<'_>) -> String {
    fill_template(ctx.lang.text(catalog_msgs(id).0), &[("{name}", "")])
        .trim()
        .to_owned()
}

fn entry<'a>(name: String, hint: Option<Msg>, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let mut line = row![card_text(name, ctx.palette)]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    if let Some(hint) = hint {
        line = line.push(card_note(ctx.lang.text(hint), ctx.palette));
    }
    dot_line(line.into(), ctx.palette)
}

fn os_block<'a>(
    os: Msg,
    entries: Vec<Element<'a, Message>>,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    column![
        page_line(ctx.lang.text(os), font_size::SMALL, ctx.palette.dim_text),
        column(entries).spacing(spacing::XS),
    ]
    .spacing(spacing::XS)
    .into()
}

fn safe_card<'a>(ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let everywhere = vec![
        entry(
            ctx.lang.text(Msg::SpaceRulesTargetFolders).to_owned(),
            Some(Msg::SpaceRulesHintCargoToml),
            ctx,
        ),
        entry(
            ctx.lang.text(Msg::SpaceRulesPackageTools).to_owned(),
            Some(Msg::SpaceRulesHintPackageCaches),
            ctx,
        ),
    ];
    let mac = vec![
        entry(catalog_name(CatalogId::DerivedData, ctx), None, ctx),
        entry(
            catalog_name(CatalogId::UnavailableSimulators, ctx),
            Some(Msg::SpaceRulesHintUnavailableSimulators),
            ctx,
        ),
        entry(catalog_name(CatalogId::Homebrew, ctx), None, ctx),
    ];
    let linux = vec![entry(
        ctx.lang
            .text(Msg::SpaceCatalogXdgCache)
            .replace("{name}", "~/.cache"),
        Some(Msg::SpaceRulesHintKnownTools),
        ctx,
    )];
    let windows = vec![entry(
        catalog_name(CatalogId::WinTempOld, ctx),
        Some(Msg::SpaceRulesHintOlderWeek),
        ctx,
    )];
    let lines = vec![
        card_text(ctx.lang.text(Msg::SpaceRulesSafeText), palette),
        os_block(Msg::SpaceRulesOsAll, everywhere, ctx),
        os_block(Msg::SpaceRulesOsMacos, mac, ctx),
        os_block(Msg::SpaceRulesOsLinux, linux, ctx),
        os_block(Msg::SpaceRulesOsWindows, windows, ctx),
        card_note(ctx.lang.text(Msg::SpaceRulesExactPaths), palette),
    ];
    info_card(
        ctx.lang.text(Msg::SpaceRulesSafeHead),
        Some(group_color(Group::Safe, palette)),
        lines,
        palette,
    )
}

fn checks_card<'a>(ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let check = |what: Msg, hint: Option<Msg>| entry(ctx.lang.text(what).to_owned(), hint, ctx);
    let lines = vec![
        card_text(ctx.lang.text(Msg::SpaceRulesChecksText), palette),
        check(Msg::SpaceRulesCheckTool, Some(Msg::SpaceRulesCheckToolHint)),
        check(Msg::SpaceRulesCheckOpen, Some(Msg::SpaceRulesCheckOpenHint)),
        check(Msg::SpaceRulesCheckSymlink, None),
        check(
            Msg::SpaceRulesCheckUnchanged,
            Some(Msg::SpaceRulesCheckUnchangedHint),
        ),
        check(
            Msg::SpaceRulesCheckWorktree,
            Some(Msg::SpaceRulesCheckWorktreeHint),
        ),
    ];
    info_card(
        ctx.lang.text(Msg::SpaceRulesChecksHead),
        None,
        lines,
        palette,
    )
}

fn how_card<'a>(ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    info_card(
        ctx.lang.text(Msg::SpaceRulesHowHead),
        None,
        vec![
            card_text(ctx.lang.text(Msg::SpaceRulesHowSafe), palette),
            card_text(ctx.lang.text(Msg::SpaceRulesHowOwn), palette),
        ],
        palette,
    )
}

fn hermes_card<'a>(ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    info_card(
        ctx.lang.text(Msg::SpaceRulesHermesHead),
        Some(group_color(Group::Decide, palette)),
        vec![
            card_text(ctx.lang.text(Msg::SpaceRulesHermesText), palette),
            dot_line(
                card_text(ctx.lang.text(Msg::SpaceRulesHermesSees), palette),
                palette,
            ),
            dot_line(
                card_text(ctx.lang.text(Msg::SpaceRulesHermesCannot), palette),
                palette,
            ),
        ],
        palette,
    )
}

fn never_card<'a>(ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    info_card(
        ctx.lang.text(Msg::SpaceRulesNeverHead),
        Some(group_color(Group::InUse, palette)),
        vec![
            dot_line(
                card_text(ctx.lang.text(Msg::SpaceRulesNeverOwn), palette),
                palette,
            ),
            dot_line(
                card_text(ctx.lang.text(Msg::SpaceRulesNeverProtected), palette),
                palette,
            ),
            card_note(ctx.lang.text(Msg::SpaceRulesNeverDecide), palette),
        ],
        palette,
    )
}

fn keys_card<'a>(ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    info_card(
        ctx.lang.text(Msg::SpaceRulesKeysHead),
        None,
        vec![
            card_text(ctx.lang.text(Msg::SpaceRulesKeysText), palette),
            key_row("Space", ctx.lang.text(Msg::SpaceRulesKeyEntry), palette),
            key_row(
                OPEN_TAB_KEY,
                ctx.lang.text(Msg::SpaceRulesKeyEverywhere),
                palette,
            ),
            key_row("F1", ctx.lang.text(Msg::SpaceRulesKeyCommands), palette),
        ],
        palette,
    )
}

/// The colours of the bars and what they stand for.
fn color_legend<'a>(ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let chip = |msg: Msg, color| color_chip(ctx.lang.text(msg), color, None::<Message>);
    row![
        page_line(
            ctx.lang.text(Msg::SpaceRulesColors),
            font_size::SMALL,
            palette.dim_text
        ),
        chip(Msg::SpaceLegendSafe, group_color(Group::Safe, palette)),
        chip(
            Msg::SpaceLegendWorktrees,
            group_color(Group::Worktrees, palette)
        ),
        chip(Msg::SpaceLegendDecide, group_color(Group::Decide, palette)),
        chip(
            Msg::SpaceRulesColorBlocked,
            group_color(Group::InUse, palette)
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center)
    .into()
}

pub fn view<'a>(ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let head = row![
        column![
            page_heading(ctx.lang.text(Msg::SpaceRulesTitle), palette),
            page_line(
                ctx.lang.text(Msg::SpaceRulesSubtitle),
                font_size::SMALL,
                palette.dim_text
            ),
        ]
        .spacing(spacing::XS),
        Space::new().width(Length::Fill),
        kit::key_button(
            ctx.lang.text(Msg::SpaceKeyBack),
            ctx.lang.text(Msg::SpaceKeyNameEscape),
            Some(Message::HideRules),
            Tone::Secondary,
            palette,
        ),
    ]
    .align_y(alignment::Vertical::Center);
    let cards = row![
        column![safe_card(ctx), how_card(ctx)]
            .spacing(spacing::XL)
            .width(Length::FillPortion(1)),
        column![
            checks_card(ctx),
            hermes_card(ctx),
            never_card(ctx),
            keys_card(ctx)
        ]
        .spacing(spacing::XL)
        .width(Length::FillPortion(1)),
    ]
    .spacing(spacing::XL);
    column![
        container(head).padding([spacing::L, spacing::XL]),
        theme::horizontal_rule(palette.border_inactive),
        container(color_legend(ctx)).padding([spacing::M, spacing::XL]),
        page_scroll_padded(
            RULES_SCROLL_ID,
            cards.into(),
            Padding::from([spacing::XL, spacing::XL])
        ),
    ]
    .height(Length::Fill)
    .into()
}
