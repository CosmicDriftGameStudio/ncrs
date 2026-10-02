# Contributing

ncrs is a small Rust project and early enough that a short conversation beats
a long document. Open an issue before a large change; that way the direction
can be agreed before the work is done.

## Before you start

- Read [ARCHITECTURE_REVIEW.md](ARCHITECTURE_REVIEW.md). It records why the
  code is shaped the way it is and which decisions are still open. A change
  that contradicts it should change that document too.
- `ROADMAP.md` holds the planned order. A feature that is not on it is
  welcome; a change that reorders it is a discussion, not a patch.

## Ground rules

- `App::update` is the only place that changes state. Views are pure
  functions of state. A commit that mutates state somewhere else will be
  asked to change.
- The `fs` layer knows nothing about the UI. It returns structured errors, and
  the UI layer turns them into text in the active language.
- Comments explain *why*, never *what*. If a line needs a comment to say what
  it does, the line is wrong.
- User-facing text belongs in `strings.json` together with its translator
  context, never inline. `build.rs` enforces the context; the tests enforce the
  translations.
- Everything that touches the filesystem runs on tokio's blocking pool, and
  long operations must be cancellable.

## Building and testing

```sh
cargo test                                    # the default feature set
cargo test --no-default-features --features gpu-with-fallback
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
cargo test readme_screenshots -- --ignored   # regenerates docs/screenshots/*.png for the README
```

CI runs the tests in every backend combination, because the backend is chosen
by cargo features and the `cfg` logic in `src/backend.rs` only compiles in
some of them.

Install `rustup` before working on this: `rust-toolchain.toml` pins the
compiler, and a Homebrew Rust ignores that pin, so a local build can silently
use a different compiler than CI.

## Commit messages

Explain what was wrong and what changed, not which lines were touched. A
reviewer wants to know why the change was necessary:

> fix: F7 raced its own reload, and the snapshot tests compared nothing

not

> refactor: update app.rs and ui_tests.rs

## Pull requests

- One change per pull request. Two unrelated fixes are two pull requests.
- Say which review findings the change addresses (`T1`, `W4`, `S2` — they are
  numbered in the review).
- If you change the UI, say what you checked in the running app. Snapshot
  tests compare rendered pixels on macOS; they are a floor, not a proof.
- If a change is deliberate about a known limitation, name the limitation in
  the commit message. The project has a habit of honest comments, and a
  commit that claims more than it does is a regression.
