# ncrs – rules for agents

These rules come before the existing code: where the code does it differently, the code is wrong and is being moved
towards them (see "Migration" below). Never copy a pattern from the code that breaks them.

## Structure: one crate per feature

- Every feature is its own crate under `crates/`, in nested TEA: its own `State`, `Message`,
  `update(&mut State, Message, &Ctx) -> Task<Message>` and `view(&State, &Ctx) -> Element<Message>`.
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

## Migration (in progress)

Most of the UI and all app logic still live in the `ncrs` crate (`src/app.rs` alone has over 30,000 lines). The
target and order, one commit per step:

1. `ncrs-ui-kit`: `src/ui/kit.rs` and `src/ui/hermes_kit.rs` merged, plus theme, format, frame, fkeys, dialog,
   context menu widget, and one file row.
2. `ncrs-search`
3. `ncrs-files` (panel, columns, sorting, hit list, history) and `ncrs-ops` (copy, move, delete, trash, pack,
   conflicts)
4. `ncrs-palette` (command palette, quick open, command line), `ncrs-settings` (settings, key bindings, config,
   themes, models), `ncrs-hermes-ui` (filing pages, review, rules, tags, overview, reorganise, reports),
   `ncrs-remote` (SFTP)
5. `ncrs` keeps only wiring: window, menu, title and status bar, message routing.

When touching a feature that has not moved yet, do not grow `src/app.rs` or `src/ui/`; move the part you touch.
