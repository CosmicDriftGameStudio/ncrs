/// Whether this build is a test: it must not reach the user's own config,
/// index or keychain.
pub fn user_machine_is_off_limits() -> bool {
    cfg!(any(test, feature = "test-support"))
}

/// Everything that finds the user's own config, index or keychain calls this
/// first: a test that got there would read or change the real data of whoever
/// runs it.
#[allow(clippy::panic, reason = "the guard is the failure, and only in tests")]
pub fn forbid_user_machine_in_tests(what: &str) {
    if user_machine_is_off_limits() {
        panic!(
            "{what} reaches the user's config, index or keychain. Tests build the app with \
             App::with_environment(Environment::isolated()) and never touch those."
        );
    }
}
