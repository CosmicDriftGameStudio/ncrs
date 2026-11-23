//! The trial and the purchase of the App Store build: a bridge to StoreKit 2
//! and the pure rule that turns what the store says into what the app allows.
//!
//! Only the build with the `storekit` feature on macOS talks to the store.
//! Every other build, including Homebrew, Linux and Windows, is never locked.
//!
//! `unsafe` lives in `bridge.rs` alone: the calls into the Swift library.

#[cfg(all(feature = "storekit", target_os = "macos"))]
mod bridge;
mod cache;
mod entitlement;
mod simulation;
mod status;
mod store;

pub use cache::{load_cached, save_cached};
pub use entitlement::{entitlement, Entitlement, FullKind, REMINDER_DAYS};
pub use simulation::Simulation;
pub use status::{Environment, StoreError, StoreStatus};
pub use store::{PurchaseOutcome, Store};

/// The free product whose purchase starts the 14-day trial.
pub const TRIAL_PRODUCT_ID: &str = "dev.cosmicdrift.ncrs.trial14";
/// The paid unlock.
pub const FULL_PRODUCT_ID: &str = "dev.cosmicdrift.ncrs.fullversion";
pub const TRIAL_DAYS: u32 = 14;

/// The first version that is locked after the trial. Anyone whose original
/// download is older got the app when it cost money and keeps it unlocked.
/// Only right if the App Store price goes to 0 with exactly this release.
pub const FIRST_LOCKED_VERSION: &str = "1.7.0";

/// Debug builds only: replaces the store with a fixed state: `new`,
/// `trial:5`, `expired`, `full`, `earlier`.
pub const SIMULATE_ENV: &str = "NCRS_STORE_SIMULATE";
/// Environment variable that replaces the made-up original version of a
/// sandbox or Xcode status. Debug builds only; a production status ignores it.
pub const ORIGINAL_VERSION_ENV: &str = "NCRS_STORE_ORIGINAL_VERSION";
