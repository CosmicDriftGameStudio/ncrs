# ncrs – rules for agents

These rules come before the existing code: where the code does it differently, the code is wrong and is being moved
towards them (see "Where code lives" below). Never copy a pattern from the code that breaks them.

## Structure: one crate per feature

- Every feature is its own crate under `crates/`, in nested TEA: its own `State`, `Message`,
  `update(&mut State, Message, &Ctx) -> (Task<Message>, Option<Action>)` (`Vec<Action>` where one message triggers several) and
  `view(&State, ViewCtx) -> Element<Message>` (`crates/ncrs-search` is the model).
  The app crate `ncrs` only wires them together (`.map(Message::Search)`), it holds no feature logic.
- Feature crates depend on `ncrs-ui-kit`, `ncrs-core` and the backend crates (`ncrs-index`, `ncrs-intake`, …), never
  on each other. Something two features need goes into the kit, `ncrs-core` or a backend crate.
- `ncrs-core` holds only plain data types several features share (`FileEntry`, `Selection`, `PanelSide`, …), no
  logic and nothing that changes often: every change to it rebuilds every feature.
- A feature gets what it needs from the app through its `Ctx` (data, resolved key bindings) and asks the app for
  anything outside itself (open a panel, show a file) by returning an `Action` from `update`, never by reaching
  into app state.
- No file over about 1,000 lines. A file that grows past it is split before anything is added.

## UI: only from the kit

- Every visible element (rows, buttons, inputs, menus, dialogs, colors, spacing) comes from `ncrs-ui-kit`.
  A feature never styles a widget itself and never builds its own row or button.
- Before building UI, look in the kit. If something is missing, add it to the kit as a component that takes what it
  shows and returns an `Element`, generic over the message type.
- The same thing looks the same everywhere: a list of files is always the kit's file row.
- Sizes that two or more features use (label columns, field widths, side panes, list heights) are tokens in
  `ncrs_ui_kit::theme::size`. A local `const` is for a size that belongs to one view alone.
- User-facing text only in `strings.json`, with translator context (see CONTRIBUTING.md).

## Building and testing locally

- Build and test only the crate you changed: `cargo check -p <crate>`, `cargo test -p <crate>`. The full workspace
  suite runs in CI, not on the developer's machine.
- At most one cargo build or test run at a time per worktree.
- Snapshot tests live in the feature crate that renders them. A pure move must keep every snapshot pixel-identical.

## Working on this repo

- Do exactly what was asked. Extra ideas are proposed, not built along.
- One `coder` per task with a precise spec; no intermediate planning agent.
- GitHub texts (issues, PRs, commits) in English, no AI attribution.

## Where code lives

The migration into feature crates is done. The crates by kind:

- Feature: `ncrs-search`, `ncrs-files`, `ncrs-ops`, `ncrs-palette`, `ncrs-settings`, `ncrs-hermes-ui`,
  `ncrs-hermes-chat`, `ncrs-remote`, `ncrs-compare`, `ncrs-filing-view`, `ncrs-monitor`, `ncrs-notes`,
  `ncrs-updates`, `ncrs-viewer`.
- Backend (no UI): `ncrs-fs`, `ncrs-index`, `ncrs-index-types` (index data types without the database), `ncrs-intake`, `ncrs-pipeline`, `ncrs-ai`, `ncrs-filing`, `ncrs-config`,
  `ncrs-jobs`, `ncrs-ocr-vision`, `ncrs-priority`, `ncrs-sandbox`, `ncrs-console`.
- Kit: `ncrs-ui-kit`. Core: `ncrs-core`. Strings: `ncrs-i18n`.

The `ncrs` crate (`src/app/`,
`src/ui/`) is wiring: window, menu, title and status bar, message routing. New feature logic goes into the matching
crate, not into `src/app/` or `src/ui/`; when you touch a part that is still there, move it.
