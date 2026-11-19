use super::*;
use crate::context_menu::ContextAction;
use crate::messages::{MenuMessage, PanelSide};
use iced::{mouse, Event, Point};

/// The pointer is placed so the image does not depend on where a test
/// runner's cursor happens to be.
fn app_with_menu() -> App {
    let mut app = App::with_fixed_panels();
    app.set_pointer_for_test(Point::new(150.0, 130.0));
    drop(app.update(Message::Files(MenuMessage::ContextMenuAt {
        side: PanelSide::Left,
        index: 2,
    })));
    app
}

#[test]
fn the_menu_is_drawn_over_the_panels() {
    let matches = simulator(&app_with_menu())
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/context_menu.png")
        .expect("snapshot comparison");
    assert!(matches, "the context menu does not match context_menu.png");
}

#[cfg(target_os = "macos")]
#[test]
fn the_menu_differs_from_the_plain_panels() {
    let same_as_panels = simulator(&app_with_menu())
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/two_panels.png")
        .expect("compare with the panels");
    assert!(!same_as_panels, "the context menu is not drawn");
}

#[test]
fn a_right_click_on_a_row_asks_for_the_menu_there() {
    let app = App::with_fixed_panels();
    let mut sim = simulator(&app);
    // Second row of the left panel: below the title and column header.
    sim.point_at(Point::new(200.0, 59.0 + 22.0 * 1.5));
    sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Right,
    ))]);
    let messages: Vec<Message> = sim.into_messages().collect();
    assert_eq!(
        messages,
        [Message::Panel(PanelMessage::RowContext {
            side: PanelSide::Left,
            index: 1
        })]
    );
}

#[test]
fn a_click_on_an_entry_runs_it() {
    let app = app_with_menu();
    let mut sim = simulator(&app);
    sim.click("Copy path").expect("the Copy path entry");
    let index = app
        .context_menu_for_test()
        .unwrap()
        .entries()
        .iter()
        .position(|entry| {
            matches!(
                entry,
                crate::context_menu::MenuEntry::Item {
                    action: ContextAction::CopyPath,
                    ..
                }
            )
        })
        .unwrap();
    let messages: Vec<Message> = sim.into_messages().collect();
    assert_eq!(
        messages,
        [Message::Files(MenuMessage::ContextMenuClick(index))]
    );
}

#[test]
fn a_greyed_out_entry_does_not_answer_a_click() {
    use crate::context_menu::MenuEntry;
    use crate::ui::layout::{MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_WIDTH};

    let mut app = App::with_fixed_panels();
    app.set_pointer_for_test(Point::new(150.0, 130.0));
    drop(app.update(Message::Files(MenuMessage::ContextMenuAt {
        side: PanelSide::Left,
        index: 1,
    })));
    let menu = app.context_menu_for_test().unwrap();
    let mut top = menu.origin().y + MENU_PADDING;
    let mut view_row = None;
    for entry in menu.entries() {
        match entry {
            MenuEntry::Separator => top += MENU_SEPARATOR_HEIGHT,
            MenuEntry::Item { action, enabled } => {
                if *action == ContextAction::View {
                    assert!(!enabled, "View is greyed out on a directory");
                    view_row = Some(top + MENU_ITEM_HEIGHT / 2.0);
                }
                top += MENU_ITEM_HEIGHT;
            }
            MenuEntry::Hermes => top += MENU_ITEM_HEIGHT,
        }
    }
    let point = Point::new(
        menu.origin().x + MENU_WIDTH / 2.0,
        view_row.expect("a View entry"),
    );
    let mut sim = simulator(&app);
    sim.point_at(point);
    sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Left,
    ))]);
    assert_eq!(sim.into_messages().count(), 0);
}
