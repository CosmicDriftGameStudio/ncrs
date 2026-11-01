//! Opening the drive menu, the favourites in it and the way out of it.

use std::path::{Path, PathBuf};

use ncrs_core::PanelSide;
use ncrs_fs::remote::ServerId;
use ncrs_fs::Volume;
use ncrs_i18n::{fill_template, Language, Msg};

use crate::action::Action;
use crate::state::Panels;
use crate::volumes::VolumeMenu;
use crate::volumes_view::{FavouriteRow, ServerRow};

/// What the app reads for the drive menu when it opens.
#[derive(Debug)]
pub struct VolumeFacts {
    pub volumes: Vec<Volume>,
    /// The saved servers, in the order of the file; `connected` is not set yet.
    pub saved_servers: Vec<ServerRow>,
    pub connected: Vec<ServerId>,
    /// What the trash row is called, `None` in a build without a trash.
    pub trash: Option<Msg>,
}

/// The servers the drive menu lists: the saved ones in the order of the
/// file, then those connected now that are not saved, by name.
pub fn server_rows(saved: Vec<ServerRow>, connected: &[ServerId]) -> Vec<ServerRow> {
    let mut rows: Vec<ServerRow> = saved
        .into_iter()
        .map(|row| ServerRow {
            connected: connected.contains(&row.server),
            ..row
        })
        .collect();
    let mut others: Vec<ServerRow> = connected
        .iter()
        .filter(|open| !rows.iter().any(|row| row.server == **open))
        .map(|server| ServerRow {
            label: server.to_string(),
            url: server.root_path(),
            server: server.clone(),
            connected: true,
        })
        .collect();
    others.sort_by(|a, b| a.label.cmp(&b.label));
    rows.extend(others);
    rows
}

fn favourite_notice(lang: Language, msg: Msg, path: &Path) -> Action {
    Action::ShowNotice(fill_template(
        lang.text(msg),
        &[("{path}", &path.display().to_string())],
    ))
}

impl Panels {
    /// Alt+F1 / Alt+F2: list the drives and highlight the one `side` is on.
    /// The favourites are listed below the drives, so the menu is worth
    /// opening with only those. `on_favourites` puts the highlight on the first.
    /// Does nothing while a prompt or dialog holds the keyboard.
    pub fn open_volume_menu_on(
        &mut self,
        side: PanelSide,
        on_favourites: bool,
        overlay_is_open: bool,
        facts_source: impl FnOnce() -> VolumeFacts,
    ) {
        if overlay_is_open {
            return;
        }
        let facts = facts_source();
        let favourites: Vec<FavouriteRow> = self
            .favourites
            .iter()
            .map(|path| FavouriteRow {
                path: path.clone(),
                present: path.is_dir(),
            })
            .collect();
        let servers = server_rows(facts.saved_servers, &facts.connected);
        if facts.volumes.is_empty() && favourites.is_empty() && servers.is_empty() {
            return;
        }
        let mut menu = VolumeMenu::new(side, facts.volumes, &self.panel(side).path)
            .with_favourites(favourites)
            .with_servers(servers)
            .with_trash(facts.trash);
        if on_favourites {
            menu.selected = menu.volumes.len();
        }
        // A panel on a server opens the menu on that server.
        if let Some(current) = self.panel(side).remote() {
            if let Some(index) = menu.servers.iter().position(|row| row.server == current) {
                menu.selected = menu.volumes.len() + menu.favourites.len() + index;
            }
        }
        self.volume_menu = Some(menu);
    }

    /// Opens the drive menu on the favourites, or says there are none.
    pub fn open_favourites(
        &mut self,
        lang: Language,
        overlay_is_open: bool,
        facts: impl FnOnce() -> VolumeFacts,
    ) -> Vec<Action> {
        if self.favourites.is_empty() {
            return vec![Action::ShowNotice(
                lang.text(Msg::StatusNoFavourites).to_owned(),
            )];
        }
        self.open_volume_menu_on(self.active, true, overlay_is_open, facts);
        Vec::new()
    }

    pub fn add_favourite(&mut self, lang: Language) -> Action {
        let path = self.active_panel().path.clone();
        let msg = if self.favourites.contains(&path) {
            Msg::StatusFavouriteAlready
        } else {
            self.favourites.push(path.clone());
            Msg::StatusFavouriteAdded
        };
        favourite_notice(lang, msg, &path)
    }

    pub fn remove_favourite(&mut self, lang: Language) -> Action {
        let path = self.active_panel().path.clone();
        let before = self.favourites.len();
        self.favourites.retain(|favourite| *favourite != path);
        let msg = if self.favourites.len() < before {
            Msg::StatusFavouriteRemoved
        } else {
            Msg::StatusFavouriteNotOne
        };
        favourite_notice(lang, msg, &path)
    }

    /// Delete or Backspace in the drive menu. A drive row ignores it.
    pub fn remove_highlighted_row(&mut self, saved_servers: &[ServerId]) -> Vec<Action> {
        let Some(menu) = self.volume_menu.as_mut() else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if let Some(index) = menu.selected_favourite() {
            let removed = menu.favourites.remove(index);
            self.favourites.retain(|path| *path != removed.path);
            menu.selected = menu.selected.min(menu.last_row());
        } else if let Some(index) = menu.selected_server() {
            // A server is closed from its row. A saved one stays in the list,
            // a server that was only connected leaves it. Removing a saved
            // one is the settings' business.
            let listed = menu.servers.get(index).cloned();
            if let Some(row) = listed.filter(|candidate| candidate.connected) {
                if saved_servers.contains(&row.server) {
                    if let Some(shown) = menu.servers.get_mut(index) {
                        shown.connected = false;
                    }
                } else {
                    menu.servers.remove(index);
                    menu.selected = menu.selected.min(menu.last_row());
                }
                actions.push(Action::Disconnect(row.server));
            }
        }
        // Without drives, the last favourite leaves nothing to pick.
        if self
            .volume_menu
            .as_ref()
            .is_some_and(|open| open.row_count() == 0)
        {
            self.volume_menu = None;
        }
        actions
    }

    /// Closes the menu and takes its panel to the highlighted drive, as any
    /// directory change does: a failing read shows its error in the status bar.
    pub fn go_to_selected_volume(&mut self, lang: Language) -> Vec<Action> {
        let Some(menu) = self.volume_menu.take() else {
            return Vec::new();
        };
        if menu.selected_trash() {
            return vec![Action::OpenTrash(menu.side)];
        }
        let target: PathBuf = match menu.volumes.get(menu.selected) {
            Some(volume) => volume.path.clone(),
            None if menu.selected_server().is_some() => {
                let Some(row) = menu
                    .selected_server()
                    .and_then(|index| menu.servers.get(index))
                else {
                    return Vec::new();
                };
                row.url.clone()
            }
            None => {
                let Some(favourite) = menu
                    .selected
                    .checked_sub(menu.volumes.len())
                    .and_then(|index| menu.favourites.get(index))
                else {
                    return Vec::new();
                };
                if !favourite.present {
                    let text = fill_template(
                        lang.text(Msg::ErrorFavouriteGone),
                        &[("{path}", &favourite.path.display().to_string())],
                    );
                    return vec![Action::ShowError(text)];
                }
                favourite.path.clone()
            }
        };
        self.active = menu.side;
        vec![
            Action::ClearMessages,
            Action::Load {
                side: menu.side,
                path: target,
                select: None,
            },
        ]
    }
}
