use std::path::Path;

use crate::open_vsx::{self, ExtensionId, FetchError, FetchedExtension, ThemeFileUrl};
use crate::theme_fetch::{Fetched, Installed, Pick};

/// The blocking part of the download: the package and the themes it offers.
pub fn download_theme(
    fetch: fn(&ExtensionId) -> Result<FetchedExtension, FetchError>,
    id: &ExtensionId,
) -> Result<Fetched, FetchError> {
    let extension = fetch(id)?;
    let themes = crate::vsix::list_themes(&extension.vsix)?;
    Ok(Fetched { extension, themes })
}

/// The blocking part of the installation: files next to the config file, and
/// the config pointing at them.
pub fn install_picked_theme(pick: &Pick, config_path: &Path) -> Result<Box<Installed>, FetchError> {
    let (fetched, theme) = pick;
    let config = crate::vsix::install_theme(
        &fetched.extension.vsix,
        theme,
        &fetched.extension.id,
        config_path,
    )?;
    Ok(Box::new(Installed {
        config,
        label: theme.label.clone(),
        license: fetched.extension.license.clone(),
    }))
}

/// The blocking part of installing a theme from a link: the download, the
/// check that it is a theme, the files and the config.
pub fn install_theme_from_link(
    link: &ThemeFileUrl,
    config_path: &Path,
) -> Result<Box<Installed>, FetchError> {
    let bytes = open_vsx::fetch_theme_file(link)?;
    let (config, label) = crate::vsix::install_theme_file(&bytes, link.file_name(), config_path)?;
    Ok(Box::new(Installed {
        config,
        label,
        license: None,
    }))
}
