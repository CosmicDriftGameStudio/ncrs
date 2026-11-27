//! The words of a catalog folder: its name and why it can go. Shared by the tab and the chat.

use ncrs_i18n::Msg;

use crate::CatalogId;

/// The name and the reason of a catalog folder. A folder of the "user own" kind has no fixed name; the caller shows its folder name.
pub fn catalog_msgs(id: CatalogId) -> (Msg, Msg) {
    match id {
        CatalogId::CargoTarget => (
            Msg::SpaceCatalogCargoTarget,
            Msg::SpaceCatalogCargoTargetWhy,
        ),
        CatalogId::Sccache => (Msg::SpaceCatalogSccache, Msg::SpaceCatalogSccacheWhy),
        CatalogId::CargoRegistryCache => (
            Msg::SpaceCatalogCargoRegistryCache,
            Msg::SpaceCatalogCargoRegistryCacheWhy,
        ),
        CatalogId::Yarn => (Msg::SpaceCatalogYarn, Msg::SpaceCatalogYarnWhy),
        CatalogId::Npm => (Msg::SpaceCatalogNpm, Msg::SpaceCatalogNpmWhy),
        CatalogId::Bun => (Msg::SpaceCatalogBun, Msg::SpaceCatalogBunWhy),
        CatalogId::Pnpm => (Msg::SpaceCatalogPnpm, Msg::SpaceCatalogPnpmWhy),
        CatalogId::Pip => (Msg::SpaceCatalogPip, Msg::SpaceCatalogPipWhy),
        CatalogId::Homebrew => (Msg::SpaceCatalogHomebrew, Msg::SpaceCatalogHomebrewWhy),
        CatalogId::DerivedData => (
            Msg::SpaceCatalogDerivedData,
            Msg::SpaceCatalogDerivedDataWhy,
        ),
        CatalogId::UnavailableSimulators => (
            Msg::SpaceCatalogUnavailableSimulators,
            Msg::SpaceCatalogUnavailableSimulatorsWhy,
        ),
        CatalogId::Gradle => (Msg::SpaceCatalogGradle, Msg::SpaceCatalogGradleWhy),
        CatalogId::AppCache(_) => (Msg::SpaceCatalogXdgCache, Msg::SpaceCatalogXdgCacheWhy),
        CatalogId::WinTempOld => (Msg::SpaceCatalogWinTempOld, Msg::SpaceCatalogWinTempOldWhy),
        CatalogId::OwnRegenerable => (Msg::SpaceDecideOther, Msg::SpaceCatalogXdgCacheWhy),
    }
}
