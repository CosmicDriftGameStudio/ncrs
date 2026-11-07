//! The permissions dialog: what the entries have now, what the user has
//! touched, and the plan that comes out of it.

pub mod view;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use ncrs_core::FileEntry;
use ncrs_fs::permissions::{ForFiles, ForFolders, PermissionEdit, PermissionPlan};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::kit::Check;

/// The nine rights in the order of the grid: owner, group, others, each read,
/// write, execute. Then setuid, setgid and sticky.
const MASKS: [u32; 12] = [
    0o400, 0o200, 0o100, 0o040, 0o020, 0o010, 0o004, 0o002, 0o001, 0o4000, 0o2000, 0o1000,
];
pub(crate) const SETUID: usize = 9;
pub(crate) const SETGID: usize = 10;
pub(crate) const STICKY: usize = 11;
/// The read-only attribute, the one box on Windows.
pub const READ_ONLY: usize = MASKS.len();

/// What the dialog needs to know of one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionTarget {
    pub path: PathBuf,
    pub is_dir: bool,
    pub mode: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub read_only: Option<bool>,
}

impl PermissionTarget {
    pub fn of(entry: &FileEntry) -> Self {
        Self {
            path: entry.path.clone(),
            is_dir: entry.is_dir,
            mode: entry.details.mode,
            owner: entry.details.owner.clone(),
            group: entry.details.group.clone(),
            read_only: entry.details.windows.map(|windows| windows.read_only),
        }
    }
}

/// The targets of the entries, without the `..` row.
pub fn targets_of(entries: &[FileEntry]) -> Vec<PermissionTarget> {
    entries
        .iter()
        .filter(|entry| !entry.is_parent)
        .map(PermissionTarget::of)
        .collect()
}

/// One box: what the entries had, what it shows now, whether the user
/// clicked it. Only a clicked box goes into the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    start: Check,
    now: Check,
    touched: bool,
}

impl Cell {
    fn new(start: Check) -> Self {
        Self {
            start,
            now: start,
            touched: false,
        }
    }

    /// A box that was mixed goes round: mixed, on, off, mixed. Back on mixed
    /// it is no longer touched, so it stays as it is on each entry.
    fn click(&mut self) {
        self.now = match (self.start, self.now) {
            (Check::Mixed, Check::Mixed) => Check::On,
            (Check::Mixed, Check::On) => Check::Off,
            (Check::Mixed, Check::Off) => Check::Mixed,
            (_, Check::On) => Check::Off,
            (_, Check::Off | Check::Mixed) => Check::On,
        };
        self.touched = self.now != Check::Mixed;
    }

    fn set(&mut self, on: bool) {
        self.now = if on { Check::On } else { Check::Off };
        self.touched = true;
    }
}

/// Owner or group over all entries.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Who {
    Unknown,
    Same(String),
    Mixed,
}

fn who_of(names: impl Iterator<Item = Option<String>>) -> Who {
    let mut known = names.flatten();
    let Some(first) = known.next() else {
        return Who::Unknown;
    };
    if known.all(|name| name == first) {
        Who::Same(first)
    } else {
        Who::Mixed
    }
}

fn aggregate(values: impl Iterator<Item = bool>) -> Check {
    let (mut on, mut off) = (false, false);
    for value in values {
        on |= value;
        off |= !value;
    }
    match (on, off) {
        (true, true) => Check::Mixed,
        (true, false) => Check::On,
        (false, _) => Check::Off,
    }
}

/// Three or four octal digits, nothing else.
fn parse_octal(text: &str) -> Option<u32> {
    let text = text.trim();
    let digits_only = text.chars().all(|digit| ('0'..='7').contains(&digit));
    if !(3..=4).contains(&text.len()) || !digits_only {
        return None;
    }
    u32::from_str_radix(text, 8).ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionsDialog {
    targets: Vec<PermissionTarget>,
    cells: Vec<Cell>,
    owner: Who,
    group: Who,
    /// What stands in the octal field while the user types; the boxes
    /// replace it as soon as one is clicked.
    typed: Option<String>,
    windows: bool,
    advanced: bool,
    contents: bool,
    for_files: ForFiles,
    for_folders: ForFolders,
}

impl PermissionsDialog {
    pub fn new(targets: Vec<PermissionTarget>, windows: bool) -> Self {
        let mut cells: Vec<Cell> = MASKS
            .iter()
            .map(|mask| {
                Cell::new(aggregate(
                    targets
                        .iter()
                        .filter_map(|target| target.mode)
                        .map(|mode| mode & mask != 0),
                ))
            })
            .collect();
        cells.push(Cell::new(aggregate(
            targets.iter().filter_map(|target| target.read_only),
        )));
        let advanced = cells
            .get(SETUID..=STICKY)
            .is_some_and(|special| special.iter().any(|cell| cell.start != Check::Off));
        Self {
            owner: who_of(targets.iter().map(|target| target.owner.clone())),
            group: who_of(targets.iter().map(|target| target.group.clone())),
            targets,
            cells,
            typed: None,
            windows,
            advanced,
            contents: false,
            for_files: ForFiles::AsAbove,
            for_folders: ForFolders::AsAbove,
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.targets
            .iter()
            .map(|target| target.path.clone())
            .collect()
    }

    pub fn has_folder(&self) -> bool {
        self.targets.iter().any(|target| target.is_dir)
    }

    pub fn is_windows(&self) -> bool {
        self.windows
    }

    pub fn advanced_open(&self) -> bool {
        self.advanced
    }

    pub fn contents(&self) -> bool {
        self.contents
    }

    pub fn for_files(&self) -> ForFiles {
        self.for_files
    }

    pub fn for_folders(&self) -> ForFolders {
        self.for_folders
    }

    /// The state of box `index`: 0 to 8 the grid by rows, then setuid,
    /// setgid, sticky, then [`READ_ONLY`].
    pub fn state(&self, index: usize) -> Check {
        self.cells.get(index).map_or(Check::Off, |cell| cell.now)
    }

    pub fn click(&mut self, index: usize) {
        if let Some(cell) = self.cells.get_mut(index) {
            cell.click();
            self.typed = None;
        }
    }

    pub fn toggle_advanced(&mut self) {
        self.advanced = !self.advanced;
    }

    pub fn toggle_contents(&mut self) {
        self.contents = !self.contents;
    }

    pub fn set_for_files(&mut self, choice: ForFiles) {
        self.for_files = choice;
    }

    pub fn set_for_folders(&mut self, choice: ForFolders) {
        self.for_folders = choice;
    }

    /// The number as the user types it. A valid one sets every box and
    /// counts them all as touched; an invalid one stays in the field only.
    pub fn type_octal(&mut self, text: String) {
        if let Some(value) = parse_octal(&text) {
            for (cell, mask) in self.cells.iter_mut().zip(MASKS) {
                cell.set(value & mask != 0);
            }
            if value & 0o7000 != 0 {
                self.advanced = true;
            }
        }
        self.typed = Some(text);
    }

    /// The rights as a number, `None` while a box is mixed.
    fn value(&self) -> Option<u32> {
        let mut value = 0;
        for (cell, mask) in self.cells.iter().zip(MASKS) {
            match cell.now {
                Check::Mixed => return None,
                Check::On => value |= mask,
                Check::Off => {}
            }
        }
        Some(value)
    }

    /// The octal field: what is typed, else the number of the boxes (four
    /// digits with a special bit), else "mixed".
    pub fn octal_text(&self, lang: Language) -> String {
        if let Some(typed) = &self.typed {
            return typed.clone();
        }
        match self.value() {
            Some(value) if value & 0o7000 != 0 => format!("{value:04o}"),
            Some(value) => format!("{value:03o}"),
            None => lang.text(Msg::PermissionsMixed).to_owned(),
        }
    }

    /// The rights spelled like `ls -l`; `None` while a box is mixed or the
    /// typed number is not valid.
    pub fn symbolic(&self) -> Option<String> {
        let value = match &self.typed {
            Some(typed) => parse_octal(typed)?,
            None => self.value()?,
        };
        let has = |mask: u32| value & mask != 0;
        let mut text = String::new();
        for (offset, special) in [(0o100, 0o4000), (0o010, 0o2000), (0o001, 0o1000)] {
            let class = [offset * 4, offset * 2, offset];
            text.push(if has(class[0]) { 'r' } else { '-' });
            text.push(if has(class[1]) { 'w' } else { '-' });
            text.push(match (has(class[2]), has(special)) {
                (true, true) if special == 0o1000 => 't',
                (false, true) if special == 0o1000 => 'T',
                (true, true) => 's',
                (false, true) => 'S',
                (true, false) => 'x',
                (false, false) => '-',
            });
        }
        Some(text)
    }

    pub fn title(&self, lang: Language) -> String {
        if let [only] = self.targets.as_slice() {
            let name = only
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            return fill_template(lang.text(Msg::PermissionsTitleOne), &[("{name}", &name)]);
        }
        let folders = self.targets.iter().filter(|target| target.is_dir).count();
        let files = self.targets.len() - folders;
        let counted = |count: usize, one: Msg, many: Msg| match count {
            0 => None,
            1 => Some(lang.text(one).to_owned()),
            _ => Some(fill_template(
                lang.text(many),
                &[("{count}", &count.to_string())],
            )),
        };
        let files = counted(
            files,
            Msg::PermissionsTitleFileOne,
            Msg::PermissionsTitleFiles,
        );
        let folders = counted(
            folders,
            Msg::PermissionsTitleFolderOne,
            Msg::PermissionsTitleFolders,
        );
        let parts = match (files, folders) {
            (Some(files), Some(folders)) => fill_template(
                lang.text(Msg::PermissionsTitlePartsJoined),
                &[("{files}", &files), ("{folders}", &folders)],
            ),
            (Some(only), None) | (None, Some(only)) => only,
            (None, None) => String::new(),
        };
        fill_template(lang.text(Msg::PermissionsTitleMany), &[("{parts}", &parts)])
    }

    /// The line with owner and group, `None` where the system knows neither.
    pub fn owner_and_group(&self, lang: Language) -> Option<String> {
        if self.owner == Who::Unknown && self.group == Who::Unknown {
            return None;
        }
        let shown = |who: &Who| match who {
            Who::Unknown => "–".to_owned(),
            Who::Same(name) => name.clone(),
            Who::Mixed => lang.text(Msg::PermissionsMixed).to_owned(),
        };
        Some(fill_template(
            lang.text(Msg::PermissionsOwnerAndGroup),
            &[
                ("{owner}", &shown(&self.owner)),
                ("{group}", &shown(&self.group)),
            ],
        ))
    }

    fn touched_bits(&self, state: Check) -> u32 {
        self.cells
            .iter()
            .zip(MASKS)
            .filter(|(cell, _)| cell.touched && cell.now == state)
            .fold(0, |bits, (_, mask)| bits | mask)
    }

    /// What Apply does; `None` when nothing was touched and nothing goes
    /// into the folders, so Apply only closes.
    pub fn plan(&self) -> Option<PermissionPlan> {
        let edit = PermissionEdit {
            set: self.touched_bits(Check::On),
            clear: self.touched_bits(Check::Off),
            read_only: self
                .cells
                .get(READ_ONLY)
                .filter(|cell| cell.touched)
                .map(|cell| cell.now == Check::On),
        };
        let recurse =
            (self.contents && self.has_folder()).then_some((self.for_files, self.for_folders));
        (edit != PermissionEdit::default() || recurse.is_some())
            .then_some(PermissionPlan { edit, recurse })
    }
}
