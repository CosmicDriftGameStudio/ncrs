//! The calls into the Swift library (swift/StoreBridge.swift). Every call
//! blocks until StoreKit answers: never call them from the main thread.

use std::ffi::{c_char, CStr, CString};

use crate::status::StoreError;

extern "C" {
    fn ncrs_store_start_listener();
    fn ncrs_store_changes() -> u64;
    fn ncrs_store_status() -> *mut c_char;
    fn ncrs_store_purchase(product_id: *const c_char) -> *mut c_char;
    fn ncrs_store_restore() -> *mut c_char;
    fn ncrs_store_free(pointer: *mut c_char);
}

#[allow(unsafe_code)]
pub fn start_listener() {
    // SAFETY: takes no input; the Swift side starts its task once and ignores
    // further calls.
    unsafe { ncrs_store_start_listener() }
}

#[allow(unsafe_code)]
pub fn changes() -> u64 {
    // SAFETY: reads a lock-protected counter, no input.
    unsafe { ncrs_store_changes() }
}

pub fn status() -> Result<String, StoreError> {
    // SAFETY: takes no input; the pointer goes straight to `take`.
    #[allow(unsafe_code)]
    take(unsafe { ncrs_store_status() })
}

pub fn restore() -> Result<String, StoreError> {
    // SAFETY: takes no input; the pointer goes straight to `take`.
    #[allow(unsafe_code)]
    take(unsafe { ncrs_store_restore() })
}

pub fn purchase(product_id: &str) -> Result<String, StoreError> {
    let id = CString::new(product_id).map_err(|error| StoreError::Malformed(error.to_string()))?;
    // SAFETY: `id` is a NUL-terminated string that outlives the call; Swift
    // copies it before returning.
    #[allow(unsafe_code)]
    take(unsafe { ncrs_store_purchase(id.as_ptr()) })
}

/// Copies the answer and gives the C string back to the Swift allocator.
#[allow(unsafe_code)]
fn take(pointer: *mut c_char) -> Result<String, StoreError> {
    if pointer.is_null() {
        return Err(StoreError::Store("no answer from the bridge".to_owned()));
    }
    // SAFETY: non-null, NUL-terminated and allocated by the bridge with
    // strdup; it is read once and freed once with the matching free function.
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    free_answer(pointer);
    Ok(text)
}

#[allow(unsafe_code)]
fn free_answer(pointer: *mut c_char) {
    // SAFETY: `pointer` came from the bridge and is not used after this.
    unsafe { ncrs_store_free(pointer) }
}
