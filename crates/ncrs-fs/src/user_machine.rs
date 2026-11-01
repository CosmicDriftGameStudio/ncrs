/// Everything that finds the user's own config, index or keychain calls this
/// first: a test that got there would read or change the real data of whoever
/// runs it.
#[allow(clippy::panic, reason = "the guard is the failure, and only in tests")]
pub fn forbid_user_machine_in_tests(what: &str) {
    if cfg!(any(test, feature = "test-support")) {
        panic!(
            "{what} reaches the user's config, index or keychain. Tests build the app with \
             App::with_environment(Environment::isolated()) and never touch those."
        );
    }
}
