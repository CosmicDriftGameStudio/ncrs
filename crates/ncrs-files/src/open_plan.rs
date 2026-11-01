//! F3 and F4: what happens to the file under the cursor, and what the status
//! bar says when the program for it cannot be started.

use std::path::PathBuf;

use ncrs_fs::remote::RemotePath;
use ncrs_fs::OpenKind;
use ncrs_i18n::{fill_template, Language, Msg};

use crate::state::Panels;

/// Where the file to open comes from.
#[derive(Debug, Clone)]
pub enum OpenSource {
    /// The search results are in front: the file under their cursor, with the
    /// line of the hit.
    Search(Option<(PathBuf, Option<u64>)>),
    /// The row under the cursor of the active panel.
    ActivePanel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenPlan {
    Nothing,
    /// The status bar says why with this text.
    Refused(Msg),
    /// A file on a server is fetched, never opened in place: the path is an
    /// address, and the program would get nothing it can open.
    ViewFromServer(RemotePath),
    /// A file inside an archive is unpacked first, and only viewing works: the
    /// copy is thrown away, so an edit would be lost.
    ViewFromArchive {
        source: PathBuf,
        archive: PathBuf,
    },
    Launch {
        path: PathBuf,
        line: Option<u64>,
    },
}

impl Panels {
    /// Tags are ignored, as in Norton Commander; `..` and directories have
    /// nothing to view or edit.
    pub fn plan_open(&self, kind: OpenKind, source: OpenSource) -> OpenPlan {
        let target = match source {
            OpenSource::Search(target) => target,
            OpenSource::ActivePanel => self
                .active_panel()
                .selected_entry()
                .filter(|entry| !entry.is_parent && !entry.is_dir)
                .map(|entry| (entry.path.clone(), None)),
        };
        let Some((path, line)) = target else {
            return OpenPlan::Nothing;
        };
        if let Some(remote) = RemotePath::parse(&path) {
            return match kind {
                OpenKind::Edit => OpenPlan::Refused(Msg::StatusRemoteReadOnly),
                OpenKind::View => OpenPlan::ViewFromServer(remote),
            };
        }
        let in_archive = ncrs_fs::archive::locate_in_archive(&path)
            .filter(|location| !location.inner.is_empty())
            .map(|location| location.archive);
        match (in_archive, kind) {
            (Some(_), OpenKind::Edit) => OpenPlan::Refused(Msg::StatusArchiveReadOnly),
            (Some(archive), OpenKind::View) => OpenPlan::ViewFromArchive {
                source: path,
                archive,
            },
            (None, _) => OpenPlan::Launch { path, line },
        }
    }
}

/// What a file handed to the viewer or editor is a copy of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOf {
    /// Nothing: it is the file itself.
    Nothing,
    Archive,
    Server,
}

/// Why no program opens `name`. `view_key` and `edit_key` are the labels of
/// the keys that view and edit, when they have one.
pub fn open_refused_text(
    lang: Language,
    name: &str,
    from: CopyOf,
    view_key: Option<String>,
    edit_key: Option<String>,
) -> String {
    match from {
        // The edit key is refused in an archive and on a server, so it is not
        // offered.
        CopyOf::Server => fill_template(
            lang.text(Msg::ErrorRemoteViewExecutable),
            &[("{name}", name)],
        ),
        CopyOf::Archive => fill_template(
            lang.text(Msg::ErrorArchiveViewExecutable),
            &[("{name}", name)],
        ),
        CopyOf::Nothing => {
            let keys: Vec<String> = [view_key, edit_key].into_iter().flatten().collect();
            if keys.is_empty() {
                fill_template(lang.text(Msg::ErrorOpenRefusedNoKey), &[("{name}", name)])
            } else {
                fill_template(
                    lang.text(Msg::ErrorOpenRefused),
                    &[("{name}", name), ("{keys}", &keys.join("/"))],
                )
            }
        }
    }
}
