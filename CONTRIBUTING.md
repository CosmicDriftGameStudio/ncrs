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

- Every feature is its own crate with its own `State`, `Message`, `update`
  and `view` (nested TEA); `App::update` only routes messages to them. Only
  `update` functions change state, views are pure functions of state. See
  `CLAUDE.md` for the crate layout and the UI kit rule.
- The `fs` layer knows nothing about the UI. It returns structured errors, and
  the UI layer turns them into text in the active language.
- Comments explain *why*, never *what*. If a line needs a comment to say what
  it does, the line is wrong.
- User-facing text belongs in `strings.json` together with its translator
  context, never inline. `crates/ncrs-i18n/build.rs` enforces the context; the tests enforce the
  translations.
- Everything that touches the filesystem runs on tokio's blocking pool, and
  long operations must be cancellable.

## Building and testing

```sh
cargo test                                    # the default feature set, gpu-with-fallback
cargo test --no-default-features --features software-rendering
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
cargo test readme_screenshots -- --ignored   # regenerates docs/screenshots/*.png for the README
```

The default build compiles llama.cpp (the built-in AI model), which needs cmake, a
C++17 compiler and clang (libclang, for bindgen). `--no-default-features --features
gpu-with-fallback` builds without it. The tests that run the model are ignored
until the file is there: `bash scripts/fetch-ai-model.sh target/ai-models`, then
`cargo test -p ncrs-ai --features llama with_model_ -- --ignored`.

CI runs the tests in every backend combination, because the backend is chosen
by cargo features and the `cfg` logic in `src/backend.rs` only compiles in
some of them.

Install `rustup` before working on this: `rust-toolchain.toml` pins the
compiler, and a Homebrew Rust ignores that pin, so a local build can silently
use a different compiler than CI.

## Changing the index schema

The index schema is the numbered SQL files in `crates/ncrs-index/src/migrations/`,
listed in `MIGRATIONS`. A change is a new step with the next number; a step that
a release has shipped is never edited, because indexes in the wild have already
run it. All pending steps run in one transaction, so a failing step leaves the
file as it was. The tests open the fixture of every earlier release
(`crates/ncrs-index/tests/fixtures`) and must keep passing; a release that
changes the schema adds a fixture written by that release, with the generator
next to it.

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
