//! The handle the app holds. Calls block: run them on the blocking pool.

use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cache::{load_cached, save_cached};
use crate::entitlement::{entitlement, Entitlement};
use crate::simulation::Simulation;
use crate::status::{StoreError, StoreStatus};
use crate::{ORIGINAL_VERSION_ENV, SIMULATE_ENV};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PurchaseOutcome {
    Purchased,
    Cancelled,
    /// Waiting for approval (Ask to Buy); the listener reports it later.
    Pending,
    Failed(String),
}

#[derive(Clone, Copy, Debug)]
enum Backend {
    /// Never locked. Not built in the store build.
    #[cfg_attr(all(feature = "storekit", target_os = "macos"), allow(dead_code))]
    Off,
    Simulated(Simulation),
    #[cfg(all(feature = "storekit", target_os = "macos"))]
    StoreKit,
}

#[derive(Clone, Debug)]
pub struct Store {
    backend: Backend,
    cache_folder: PathBuf,
}

impl Store {
    /// The backend follows the build: StoreKit in the store build on macOS,
    /// otherwise nothing. `NCRS_STORE_SIMULATE` replaces it in debug builds
    /// only: a release build must not let an environment variable lift the lock.
    pub fn new(cache_folder: PathBuf) -> Self {
        let simulate = env::var(SIMULATE_ENV).ok();
        Self {
            backend: backend_for(simulate.as_deref(), cfg!(debug_assertions)),
            cache_folder,
        }
    }

    /// Starts the transaction listener of StoreKit. Once at startup.
    pub fn start(&self) {
        #[cfg(all(feature = "storekit", target_os = "macos"))]
        if matches!(self.backend, Backend::StoreKit) {
            crate::bridge::start_listener();
        }
    }

    /// Counts transactions that arrived on their own (Ask to Buy approved,
    /// bought on another Mac); reload when it changes.
    pub fn changes(&self) -> u64 {
        match self.backend {
            #[cfg(all(feature = "storekit", target_os = "macos"))]
            Backend::StoreKit => crate::bridge::changes(),
            Backend::Off | Backend::Simulated(_) => 0,
        }
    }

    pub fn status(&self) -> Result<StoreStatus, StoreError> {
        match self.backend {
            Backend::Off => Ok(StoreStatus {
                unsupported: true,
                ..StoreStatus::default()
            }),
            Backend::Simulated(simulation) => Ok(simulation.status(now_unix())),
            #[cfg(all(feature = "storekit", target_os = "macos"))]
            Backend::StoreKit => StoreStatus::from_json(&crate::bridge::status()?),
        }
    }

    /// Whether this build has anything to ask: false in every build that is
    /// never locked.
    pub fn is_active(&self) -> bool {
        !matches!(self.backend, Backend::Off)
    }

    /// What the last good answer allows, without asking the store; for the
    /// time until the first answer is there.
    pub fn cached_entitlement(&self) -> Entitlement {
        match self.backend {
            Backend::Off => Entitlement::Unrestricted,
            Backend::Simulated(_) => self.entitlement(),
            #[cfg(all(feature = "storekit", target_os = "macos"))]
            Backend::StoreKit => self.decide(Err(StoreError::Store("not asked yet".to_owned()))),
        }
    }

    /// Asks the store, remembers a good answer and decides.
    pub fn entitlement(&self) -> Entitlement {
        self.decide(self.status())
    }

    #[cfg_attr(
        not(all(feature = "storekit", target_os = "macos")),
        allow(unused_variables)
    )]
    pub fn purchase(&self, product_id: &str) -> PurchaseOutcome {
        match self.backend {
            Backend::Off => PurchaseOutcome::Failed("no store in this build".to_owned()),
            Backend::Simulated(_) => PurchaseOutcome::Purchased,
            #[cfg(all(feature = "storekit", target_os = "macos"))]
            Backend::StoreKit => match crate::bridge::purchase(product_id) {
                Ok(text) => parse_outcome(&text),
                Err(error) => PurchaseOutcome::Failed(error.to_string()),
            },
        }
    }

    /// Syncs with the App Store, then reads the status again.
    pub fn restore(&self) -> Entitlement {
        match self.backend {
            #[cfg(all(feature = "storekit", target_os = "macos"))]
            Backend::StoreKit => {
                let fetched =
                    crate::bridge::restore().and_then(|text| StoreStatus::from_json(&text));
                self.decide(fetched)
            }
            Backend::Off | Backend::Simulated(_) => self.entitlement(),
        }
    }

    fn decide(&self, fetched: Result<StoreStatus, StoreError>) -> Entitlement {
        if let Ok(status) = &fetched {
            // A failed write only loses the offline fallback.
            let _ignored = save_cached(&self.cache_folder, status);
        }
        // Debug builds only, and never for a production status (see `entitlement`).
        let override_version = if cfg!(debug_assertions) {
            env::var(ORIGINAL_VERSION_ENV).ok()
        } else {
            None
        };
        entitlement(
            fetched,
            load_cached(&self.cache_folder),
            now_unix(),
            override_version.as_deref(),
        )
    }
}

fn backend_for(simulate: Option<&str>, debug_build: bool) -> Backend {
    match simulate.filter(|_| debug_build).and_then(Simulation::parse) {
        Some(simulation) => Backend::Simulated(simulation),
        None => default_backend(),
    }
}

fn default_backend() -> Backend {
    #[cfg(all(feature = "storekit", target_os = "macos"))]
    return Backend::StoreKit;
    #[cfg(not(all(feature = "storekit", target_os = "macos")))]
    return Backend::Off;
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(any(test, all(feature = "storekit", target_os = "macos")))]
fn parse_outcome(text: &str) -> PurchaseOutcome {
    use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};

    let Ok(Some(JsonValue::Object(object))) = parse_to_value(text, &ParseOptions::default()) else {
        return PurchaseOutcome::Failed(format!("malformed answer: {text}"));
    };
    match object.get_string("outcome").map(|outcome| outcome.as_ref()) {
        Some("purchased") => PurchaseOutcome::Purchased,
        Some("cancelled") => PurchaseOutcome::Cancelled,
        Some("pending") => PurchaseOutcome::Pending,
        _ => PurchaseOutcome::Failed(
            object
                .get_string("error")
                .map_or_else(|| "unknown outcome".to_owned(), ToString::to_string),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_purchase_outcome() {
        assert_eq!(
            parse_outcome(r#"{"outcome":"purchased"}"#),
            PurchaseOutcome::Purchased
        );
        assert_eq!(
            parse_outcome(r#"{"outcome":"cancelled"}"#),
            PurchaseOutcome::Cancelled
        );
        assert_eq!(
            parse_outcome(r#"{"outcome":"pending"}"#),
            PurchaseOutcome::Pending
        );
        assert_eq!(
            parse_outcome(r#"{"outcome":"failed","error":"no network"}"#),
            PurchaseOutcome::Failed("no network".to_owned())
        );
        assert!(matches!(parse_outcome("???"), PurchaseOutcome::Failed(_)));
    }

    #[test]
    fn the_simulation_needs_a_debug_build() {
        assert!(matches!(
            backend_for(Some("expired"), true),
            Backend::Simulated(Simulation::Expired)
        ));
        assert!(!matches!(
            backend_for(Some("expired"), false),
            Backend::Simulated(_)
        ));
        assert!(!matches!(
            backend_for(Some("nonsense"), true),
            Backend::Simulated(_)
        ));
    }

    #[test]
    fn a_build_without_a_store_is_never_locked() {
        let Ok(folder) = tempfile::tempdir() else {
            return;
        };
        let store = Store {
            backend: Backend::Off,
            cache_folder: folder.path().to_path_buf(),
        };
        assert_eq!(store.entitlement(), Entitlement::Unrestricted);
        assert_eq!(store.changes(), 0);
    }
}
