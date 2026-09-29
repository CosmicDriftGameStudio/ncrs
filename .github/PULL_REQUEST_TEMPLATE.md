## What this changes

<!-- One or two sentences. What was wrong or missing, and what it does now. -->

## Why

<!-- The problem this solves. If it addresses a numbered finding from
     ARCHITECTURE_REVIEW.md, name it here: "T1", "W4", "S2". -->

## How it was verified

<!-- Tests you ran, and — if the UI changed — what you checked in the
     running app. Snapshot tests compare pixels on macOS only; they are a
     floor, not a proof. -->

- [ ] `cargo test` and the three feature combinations in `ci.yml` pass
- [ ] `cargo clippy --all-targets -- -D warnings` is clean
- [ ] `cargo fmt --all --check` is clean
- [ ] `ROADMAP.md` updated, if the change moves the plan
- [ ] `CHANGELOG.md` updated, if the change is user-visible

## Known limitations

<!-- Anything this deliberately does not do. A commit that claims more than
     it does is worse than one that names its edges. -->
