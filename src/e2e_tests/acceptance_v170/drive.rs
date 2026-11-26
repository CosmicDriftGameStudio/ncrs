//! What the flows share: looking at what a run left on disk, in the index and
//! in the config.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use iced::advanced::widget::operation::Outcome;
use iced::advanced::widget::{Id, Operation};
use iced::{Rectangle, Vector};

use super::probe::World;
use crate::e2e_tests::acceptance_v160::{shoot_sized, Live, SHOT_SIZE};
use crate::e2e_tests::Session;

pub const SHOT_FOLDER: &str = "target/acceptance/v170";

/// A picture of the whole window at the size of the other acceptance pictures.
pub fn shot<P: Live>(session: &Session<P>, name: &str) {
    shoot_sized(&session.app, (SHOT_FOLDER, name), SHOT_SIZE, |_| {});
}

pub fn text_of(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{path:?}: {err}"))
}

pub fn has_open_review(world: &World, path: &Path) -> bool {
    let index = world.index();
    index
        .document_by_path(path)
        .unwrap()
        .is_some_and(|document| index.open_review_of(document.id).unwrap().is_some())
}

pub fn open_reviews(world: &World) -> usize {
    world.index().open_reviews().unwrap().len()
}

pub fn files_below(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut open = vec![folder.to_path_buf()];
    while let Some(next) = open.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                open.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

/// Whether a journal item moved `from` somewhere.
pub fn journal_moved(world: &World, from: &Path) -> Option<PathBuf> {
    let index = world.index();
    index
        .recent_batches(500)
        .unwrap()
        .into_iter()
        .flat_map(|(_, items)| items)
        .find(|item| item.from == from)
        .map(|item| item.to)
}

/// No flow changes a setting: the inbox is still a watched source and the
/// archive its target.
pub fn config_intact(world: &World) {
    let saved = std::fs::read_to_string(world.root.join("config.toml")).unwrap();
    for wanted in [
        "when = \"watch\"".to_owned(),
        "id = \"main\"".to_owned(),
        format!("{:?}", world.inbox_dir),
        format!("{:?}", world.archive_dir),
    ] {
        assert!(saved.contains(&wanted), "{wanted} is gone: {saved}");
    }
}

/// What a look at the sidebar finds: whether the chosen row lies in the
/// window of the scrollable and how far it is scrolled.
#[derive(Default)]
pub struct SidebarLook {
    viewport: Option<(Rectangle, Rectangle, Vector)>,
    row: Option<Rectangle>,
}

impl SidebarLook {
    pub fn scrolled(&self) -> f32 {
        self.viewport
            .map_or(0.0, |(_, _, translation)| translation.y)
    }

    pub fn row_in_view(&self) -> Option<bool> {
        let ((viewport, content, translation), row) = self.viewport.zip(self.row)?;
        let top = row.y - content.y;
        Some(top >= translation.y && top + row.height <= translation.y + viewport.height)
    }
}

struct LookAtSidebar(Arc<Mutex<SidebarLook>>);

impl Operation for LookAtSidebar {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&Id::new("nav-list-chosen")) {
            self.0.lock().unwrap().row = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&Id::new("nav-list-scroll")) {
            self.0.lock().unwrap().viewport = Some((bounds, content_bounds, translation));
        }
    }

    fn finish(&self) -> Outcome<()> {
        Outcome::None
    }
}

pub fn look_at_sidebar<P: Live>(session: &mut Session<P>) -> SidebarLook {
    let look = Arc::new(Mutex::new(SidebarLook::default()));
    session.operate(Box::new(LookAtSidebar(Arc::clone(&look))));
    let found = std::mem::take(&mut *look.lock().unwrap());
    found
}
