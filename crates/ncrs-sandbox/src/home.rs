use std::path::PathBuf;

/// The home folder from the user database, which is the real one even in the
/// App Sandbox, where `$HOME` names the container. `None` when the database has
/// no entry for the current user.
pub fn real_home() -> Option<PathBuf> {
    nix::unistd::User::from_uid(nix::unistd::Uid::current())
        .ok()
        .flatten()
        .map(|user| user.dir)
        .filter(|dir| dir.is_absolute())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn the_home_of_the_user_database_is_an_absolute_path() {
        let home = real_home().expect("the test user has a database entry");
        assert!(home.is_absolute(), "{home:?}");
    }
}
