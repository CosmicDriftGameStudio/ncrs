//! Prints what the StoreKit bridge answers. Needs `--features storekit` on macOS.

#[cfg(all(feature = "storekit", target_os = "macos"))]
fn main() {
    let store = ncrs_store::Store::new(std::env::temp_dir().join("ncrs-store-probe"));
    store.start();
    match store.status() {
        Ok(status) => println!("{}", status.to_json()),
        Err(error) => println!("status error: {error}"),
    }
    println!("changes: {}", store.changes());
}

#[cfg(not(all(feature = "storekit", target_os = "macos")))]
fn main() {
    println!("build with --features storekit on macOS");
}
