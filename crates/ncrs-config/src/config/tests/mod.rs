use super::setting_writes::*;
use super::*;

struct SetupFolders {
    base: tempfile::TempDir,
    inbox: PathBuf,
    archive: PathBuf,
    target: PathBuf,
}

mod loading_and_filing;
mod paths_and_servers;
mod writing;

use self::loading_and_filing::*;
use self::paths_and_servers::*;
