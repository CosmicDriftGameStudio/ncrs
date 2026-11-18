## What this changes

<!-- One or two sentences. What was wrong or missing, and what it does now. -->

## Why

<!-- The problem this solves. Link the issue if there is one. -->

## How it was verified

<!-- Tests you ran, and — if the UI changed — what you checked in the
     running app. Snapshot tests compare pixels on macOS only; they are a
     floor, not a proof. -->

- [ ] `cargo test -p <crate>` passes for every crate the change touches
- [ ] `cargo clippy -p <crate> --all-targets -- -D warnings` is clean
- [ ] `cargo fmt --all --check` is clean
- [ ] `PRIVACY.md` and `SECURITY.md` updated, if the change opens a connection or stores new data
- [ ] `CHANGELOG.md` updated, if the change is user-visible

## Known limitations

<!-- Anything this deliberately does not do. A commit that claims more than
     it does is worse than one that names its edges. -->
