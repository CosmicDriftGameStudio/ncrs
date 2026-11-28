use std::path::Path;

use crate::{FolderUse, UnknownWhy};

pub fn folder_use(_folder: &Path) -> FolderUse {
    FolderUse::Unknown(UnknownWhy::NotAllowed)
}
