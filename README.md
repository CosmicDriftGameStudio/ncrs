# NC-rs

[![CI](https://github.com/CosmicDriftGameStudio/ncrs/actions/workflows/ci.yml/badge.svg)](https://github.com/CosmicDriftGameStudio/ncrs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/CosmicDriftGameStudio/ncrs?label=release)](https://github.com/CosmicDriftGameStudio/ncrs/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.90%2B-orange.svg)](rust-toolchain.toml)

A fast, keyboard-first **dual-panel file manager inspired by Norton Commander**, written in Rust
with [iced](https://iced.rs) 0.14 (GPU-accelerated, cross-platform native UI).

![NC-rs with two panels](docs/screenshots/main.png)

The architecture follows the principles of editors like Zed: async I/O, message-driven
state, pure views, and a strict separation between UI components, filesystem layer and
application state.

This is a **work in progress**. The dual-panel navigator, the file operations and
localization with per-string translator context work, and there is a
[config file](#configuration) with programs and keys. Archive support, network mounts
and theming are planned but not built — see [ROADMAP.md](ROADMAP.md)
for the state, the licensing audit and the reasoning behind the order.

## Screenshots

![The drive menu on Alt+F1](docs/screenshots/drive-menu.png)

The drive menu (`Alt+F1` for the left panel, `Alt+F2` for the right).

![The context menu](docs/screenshots/context-menu.png)

The context menu: right-click a row (`Ctrl`+click on a Mac), tap `Option` on its
own, or press `Shift`+`F10`. It acts on the tagged rows, else the cursor row.

![The search view](docs/screenshots/search.png)

The search (`Alt`+`F7` or `Cmd`+`Shift`+`F`, `Ctrl`+`Shift`+`F` on Linux and
Windows) takes the place of the active panel and searches below its directory.

![The delete dialog](docs/screenshots/delete.png)

F8 asks before it moves the tagged rows to the trash.

![The German interface](docs/screenshots/main-de.png)

The same window in German, switched with F9.

## Features

- Two side-by-side file panels (left starts in the working directory, or `$HOME` when launched from Finder; right at `$HOME`)
- Multi-selection: tag rows with `Insert` or `Space` (or `Shift`+`Up`/`Down`, or Cmd/Ctrl-click), an operation applies to the tagged rows, or to
  the single row under the cursor when nothing is tagged
- Keyboard navigation, selection highlighting, auto-scrolling
- Name / Size / Modified columns, directories first, case-insensitive sort
- `..` entry to go up; the directory you came from is re-selected
- Async directory loading (tokio blocking pool) – the UI never blocks
- Stale-result protection (fast navigation cannot show an outdated listing)
- Errors (e.g. *Permission denied*) are shown in the status bar – no panics
- Confirmations (e.g. *Path copied*) appear at the right of the title bar for three seconds
- Mouse click selects a row and activates its panel; the wheel and the trackpad scroll the panel under the pointer
- Drive menu (`Alt`+`F1` / `Alt`+`F2`): jump a panel to the system volume, home, iCloud Drive and
  cloud storage folders such as Google Drive, OneDrive or Dropbox (macOS), mounted volumes or
  drive letters, the same places as in the Finder sidebar; see [Drives on macOS](#drives-on-macos)
- Search in file names and contents below a directory, in the panel, the way
  VS Code does it: see [Search](#search)
- Localized UI (English/German) with a translator note on every string, so the UI can
  be translated mechanically without reading the code
- Runs on Linux, macOS and Windows

## Keyboard shortcuts

The defaults. Most of them can be changed in the config file, see [Keys](#keys).

| Key                | Action                                   |
|--------------------|------------------------------------------|
| `↑` / `↓`          | Move selection                           |
| `PgUp` / `PgDn`    | Move selection by one page (`Fn`+`↑`/`↓` on a Mac)  |
| `Cmd`+`↑` / `Cmd`+`↓` | Page up / down (`Ctrl` on Linux and Windows) |
| `Home` / `End`     | First / last entry                       |
| `Enter`            | Open directory (files: placeholder)      |
| Any letter or digit | Jump to the first entry whose name starts with what you type (a character within one second extends the prefix, `Q` included) |
| `Insert` / `Space` | Tag / untag the row, move down one      |
| `Shift`+`Up`/`Down`| Tag / untag the row, move up / down     |
| `*`                | Tag every row (except `..`)             |
| `Ctrl`+`*`         | Clear all tags                          |
| `Option`+`Cmd`+`C` / `Ctrl`+`Alt`+`C` | Copy the path of the tagged rows (else the cursor row) to the clipboard |
| `Ctrl`+`Cmd`+`C` / `Ctrl`+`Shift`+`C` | Copy the name of the tagged rows (else the cursor row) to the clipboard |
| Click the path in the status bar | Edit the path (`Enter` goes there, `~` is home, `Esc` puts the current path back); the icon beside it copies the folder path |
| `F3` / `F4`        | View / edit the file under the cursor in an external program (programs configurable, see [Configuration](#configuration)) |
| `F5` / `F6`        | Copy / move to the other panel          |
| `F7`               | Create directory                        |
| `F8`               | Delete to the trash, after a confirmation |
| `Shift`+`F8`       | Delete permanently (`Enter` cancels; `Shift`+`F8` again confirms) |
| `Alt`+`F1` / `Alt`+`F2` | Drive menu for the left / right panel (`↑`/`↓`, `Home`/`End`, `Enter` goes, `Esc` closes; `Option` on macOS) |
| Right click / `Shift`+`F10` / `Option` tapped alone | Context menu at the pointer or the cursor row (`↑`/`↓`, `Home`/`End`, `Enter` runs, `Esc` closes; `Ctrl`+click also on a Mac) |
| Click a column title | Sort that panel by Name / Size / Modified; a second click reverses the order (▲ / ▼). `..` and folders stay on top |
| `Ctrl`+`F3` / `Ctrl`+`F5` / `Ctrl`+`F6` (macOS: `Cmd`+`1` / `Cmd`+`3` / `Cmd`+`2`) | Sort the active panel by name / modification time / size; again reverses. macOS uses `Cmd`+digit because the system takes `Ctrl`+`F`-keys |
| `Alt`+`F7` / `Cmd`+`Shift`+`F` (`Ctrl`+`Shift`+`F` on Linux and Windows) | Search below the directory of the active panel; pressed again, it moves to the next field. See [Search](#search) |
| `Cmd`+`P` / `Ctrl`+`P` | Quick Open: jump to a file by a few letters of its path. See [Quick Open](#quick-open) |
| `Cmd`+`Shift`+`D` / `Ctrl`+`Shift`+`D`, or click the filing progress in the status bar | Filing status page (filing module on): pipeline state, load, workers, index figures and the last errors. `P` pauses or resumes, `1`/`2`/`3` set the load gentle / balanced / full for this session, `+`/`-` change the worker count, `↑`/`↓` and `Enter` show the file of an error, `Esc` closes |
| `Backspace`        | Go to parent directory                   |
| `Tab`              | Switch active panel                      |
| `F9`               | Switch language for this session (start: `lang`) |
| `F10` / `Cmd`+`Q` / `Ctrl`+`Q` | Quit                                     |

## Search

`Alt`+`F7` or `Cmd`+`Shift`+`F` (`Ctrl`+`Shift`+`F` on Linux and Windows) opens
the search in the active panel. It searches below that panel's directory, starts
as you type and stops a search still running when you type on. `Esc` brings the
panel back; the next search starts with the same fields.

- **Aa**, **ab** and **.\*** switch on case, whole words and regular
  expressions, also by keyboard: `Alt`+`C`, `Alt`+`W` and `Alt`+`R`, in the
  fields and in the results. On a Mac layout that types letters with `Option`
  plus `C`, `W` or `R` (Polish Pro: `ć`, for one), those letters cannot be
  typed into the search fields unless `search_case`, `search_whole_word` and
  `search_regex` are bound to other keys. `AltGr` on Windows and Linux is not
  affected.
- **Files** and **Exclude** take patterns separated by `;`. A pattern without
  `/` matches the name at any depth (`*.rs`), one with `/` the path below the
  search directory (`src/*.rs`). A trailing `/` means a directory with
  everything in it (`target/`). `*.*` is every file, as in Norton Commander.
- With no search text, **Files** alone lists the matching names.
- Hidden files are searched too; `.gitignore` is not read. Binary files and
  cloud files that are not downloaded are skipped, and the search stops after
  20,000 matching lines.
- `↑`/`↓`, `PgUp`/`PgDn` and `Home`/`End` move through the results, `←`/`→`
  fold and unfold a file. Typing goes to the search text, except `Space` and
  `*`, which tag (see below).
- `Enter` on a file shows it in the other panel. `Enter` on a line, or `F4`,
  opens the editor at that line, see [Configuration](#configuration).
- `Insert` or `Space` tags the file under the cursor and moves to the next file,
  `Shift`+`↑`/`↓` tag and move either way, `*` tags all files found and
  `Ctrl`+`*` clears the tags. `F5`, `F6` and `F8` copy, move and delete the
  tagged files, else the file under the cursor.

## Quick Open

`Cmd`+`P` (`Ctrl`+`P` on Linux and Windows) opens a box over the panels, as in
VS Code. Type letters of a file's path and the best matches are listed with the
matching characters highlighted: `cfgrs` finds `src/config.rs`. It lists the
files below the active panel's directory, hidden ones too and without following
links, up to 250,000 files (the status line says when it stopped there).
`↑`/`↓` pick a hit, `Enter` (or a click) shows its directory in the active panel
with the cursor on the file, and `Esc` closes the box.

## Install

Pre-built binaries for Linux, macOS and Windows are attached to each
[release](../../releases). On Linux and macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/CosmicDriftGameStudio/ncrs/main/install.sh | sh
```

On macOS, Homebrew is the better choice: it installs `ncrs.app` into
Applications (Launchpad, Dock, Spotlight) and puts `ncrs` on your `PATH`:

```bash
brew install --cask cosmicdriftgamestudio/ncrs/ncrs
```

The cask lives in the tap
[`CosmicDriftGameStudio/homebrew-ncrs`](https://github.com/CosmicDriftGameStudio/homebrew-ncrs).
It is not in homebrew-cask upstream.

Started from Finder, the Dock or Launchpad, the left panel opens in `$HOME`
instead of `/`. From a terminal it still opens in the working directory.

`install.sh` installs into `~/.local/bin`, verifies the download checksum,
needs no `sudo`, and is safe to re-run. Remove it again with:

```bash
curl -fsSL https://raw.githubusercontent.com/CosmicDriftGameStudio/ncrs/main/install.sh | sh -s -- --uninstall
```

Set `NCRS_BIN_DIR` to install elsewhere, or `NCRS_REPO` if you fork it.

On Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/CosmicDriftGameStudio/ncrs/main/install.ps1 | iex
```

## Drives on macOS

macOS guards network volumes (including macFUSE mounts such as Cryptomator), removable
volumes, iCloud Drive and cloud storage folders (`~/Library/CloudStorage`) with its privacy
settings. A refused read shows "macOS blocks access" in the status bar; clicking it opens
System Settings > Privacy & Security > Full Disk Access.

- Whose permission counts: `ncrs.app` started from the Finder or Dock asks for itself.
  Started from a terminal (`ncrs`, `cargo run`), the terminal app (Terminal, iTerm, ...)
  needs the grant, not ncrs.
- Full Disk Access covers all of these at once. Under Files & Folders macOS lists single
  switches (such as "Network Volumes") only once the app has asked.
- Restart the terminal after granting.
- `tccutil reset SystemPolicyNetworkVolumes dev.cosmicdrift.ncrs` resets an earlier
  "Don't Allow" for `ncrs.app`.

## Configuration

ncrs reads an optional TOML file:

| System | Location |
|---|---|
| macOS | `~/.config/ncrs/config.toml` (`XDG_CONFIG_HOME` is ignored, so a start from the Dock and from a terminal read the same file) |
| Linux and other Unix | `$XDG_CONFIG_HOME/ncrs/config.toml` if set and absolute, else `~/.config/ncrs/config.toml` |
| Windows | `%APPDATA%\ncrs\config.toml` |

```toml
version = 1
# Start language: "en" or "de". F9 switches it for the running session only;
# ncrs never writes the config file.
lang = "de"

[open]
# F3. Without {path} the file is added as the last argument.
view = ["open"]
# F4, and F3 on files that could run (scripts, apps): must be a text editor.
edit = ["open", "-t"]
# To open a search result at its line instead:
# edit = ["code", "--goto", "{path}:{line}"]
```

- The file is optional. Without it, or without a key, the system's own programs
  apply (the ones in the example are the macOS defaults).
- Unknown keys are errors, so a typo cannot go unnoticed. On any error the whole
  file is ignored: ncrs starts with the defaults and shows the file and line in
  the status bar until `Esc` dismisses it.
- A program is a name looked up in `PATH` or an absolute path, followed by its
  arguments. No shell is involved. A relative path such as `bin/edit` is
  rejected, because it would depend on where ncrs was started.
- An app started from the Dock or Finder has a minimal `PATH`: give Homebrew or
  VS Code command-line tools by absolute path, or use
  `["open", "-a", "Visual Studio Code"]`.
- `{path}` and `{line}` in an argument are replaced by the file and the line
  number. Without `{path}` the file is added as the last argument. `{line}` is
  the line of a search result, and `1` for `F3` and `F4` in a panel. A program
  without `{line}` opens a search result at the top: the macOS default
  `open -t` cannot go to a line, `code --goto {path}:{line}`,
  `subl {path}:{line}` and `zed {path}:{line}` can.

### Keys

`[keys]` rebinds actions. Each entry names an action and gives one key or a list
of keys:

```toml
[keys]
# F2 copies, F5 does nothing any more.
copy = "F2"
# Several keys for one action.
quit = ["F10", "primary+q"]
# No key at all; the context menu still offers it.
delete_permanently = []
# A key that belongs to another action has to be freed there too:
# F3 and F4 swapped.
view = "F4"
edit = "F3"
```

- An entry replaces all keys of that action (at most 16); actions not listed keep theirs.
  The function key bar and the context menu show the keys in effect.
- A key is modifiers and a key name joined by `+`, case does not matter:
  `primary` (`Cmd` on macOS, `Ctrl` elsewhere), `ctrl`, `alt` (or `option`),
  `shift`, `cmd` (or `super`); `F1`–`F12`, `Enter`, `Tab`, `Backspace`, `Space`,
  `Insert`, `Delete`, `Home`, `End`, `PageUp`, `PageDown`, `Up`, `Down`, `Left`,
  `Right`, `plus` for `+`, or a single character such as `*` or `c`.
- One key, two actions in one area is an error, also when the other action only
  has the key by default: say what the other action gets instead, or `[]`. The
  panel and the search count apart. `search_case`, `search_whole_word` and
  `search_regex` work only in the search; `quit`, `search`, `view`, `edit`,
  `copy`, `move`, `delete`, `delete_permanently`, `abort_job`, `switch_language`,
  `switch_panel` and the tag actions in both; the rest only in the panel.
  `primary+q` and `ctrl+q` are the same key on Linux and Windows, so a file can
  be fine on a Mac and conflict elsewhere.
- Not allowed: `Escape` (it closes dialogs and menus and stops jobs); letters and
  digits without `ctrl`, `alt`, `cmd` or `primary`, because they jump to a file
  name; `shift` with a character that is not a letter (`shift+8` arrives as `*`,
  write `*`).
- Fixed: the keys inside dialogs, menus, the name prompt and the path field
  (`Enter`, `Esc`, arrows, `Tab`), and the `Option` tap for the context menu.
  In the search `Enter`, the arrows, `PageUp`, `PageDown`, `Home` and `End`, also
  with `shift`, cannot be given to the three search options.
  In the path field only a `quit` key on an F-key, with `cmd`, or with `ctrl`
  but not `alt` gets through (`alt` and `AltGr` type characters there).
- `alt` (Option) with a letter follows the US key positions on a Mac, so on
  a German or French layout `alt+y` is the key labelled Z. A character bound
  without a modifier, such as `*`, no longer reaches type-ahead.

Actions: `quit`, `open`, `go_up`, `switch_panel`, `switch_language`,
`cursor_up`, `cursor_down`, `page_up`, `page_down`, `first`, `last`,
`toggle_tag`, `tag_up`, `tag_down`, `tag_all`, `clear_tags`, `view`, `edit`,
`copy`, `move`, `mkdir`, `delete`, `delete_permanently`, `abort_job`,
`copy_path`, `copy_name`, `drive_menu_left`, `drive_menu_right`,
`sort_by_name`, `sort_by_modified`, `sort_by_size`, `context_menu`, `search`, `quick_open`, `search_case`,
`search_whole_word`, `search_regex`, `filing_status`. Their
default keys are in [Keyboard shortcuts](#keyboard-shortcuts).

### Colors

`[theme]` picks a ready-made palette and changes single colors on top of it:

```toml
[theme]
# "default" (the dark teal look) or "classic" (Norton Commander blue).
preset = "classic"
# Then single roles, as "#rrggbb" or "#rgb".
directory = "#ffff55"
tagged = "#a50"
```

| Role | What it colors |
|---|---|
| `background` | The window around the panels and behind the function keys |
| `panel` | The file panels |
| `text` | File names and plain text, the function key digits |
| `dim_text` | Secondary text: inactive panel title, untagged star, greyed-out menu entries |
| `directory` | Directory names |
| `cursor` | The cursor row, the highlighted menu entry, the default button, the function key labels |
| `cursor_text` | Text on the cursor row, on the title bars and on the function key labels |
| `tagged` | Tagged rows, in both panels |
| `border` | The frame of the active panel, of dialogs and menus |
| `bar` | The top bar and the title of the active panel |
| `bar_inactive` | Title and frame of the inactive panel |
| `status_bar` | The status bar |
| `dialog` | Dialogs |
| `menu` | The context menu |
| `accent` | Column titles, the path, dialog titles, tag stars, menu shortcuts |
| `success` | Confirmations in the title bar |
| `error` | Errors |

- Roles not listed keep the preset's colors; without `[theme]` nothing changes.
  The order in the file does not matter, `preset` always applies first.
- An unknown role or preset and a color in another form (names, `#rrggbbaa`) are
  errors with the line, handled like any other config error.
- A color that makes text hard to read (text on `panel`, `status_bar`, `dialog`
  or `menu`, directory names on `panel`, `cursor_text` on `cursor`, below 3:1) is
  applied anyway; the status bar says which pair and where, until `Esc`.
- Colors are read at start; restart ncrs after editing the file.

## What ncrs encrypts

This applies to the filing module (`[modules.filing] enabled = true`). With the
module off, ncrs opens and creates no index.

ncrs is a file manager with a convenience index for your documents. It is not a
high-security application, and it does not try to be one.

**Encrypted:** the index, `index.db` in the data directory
(`~/Library/Application Support/ncrs` on macOS, `$XDG_DATA_HOME/ncrs` or
`~/.local/share/ncrs` on Linux, `%APPDATA%\ncrs` on Windows). It is a SQLCipher
database (AES-256). The key is kept in the operating system's credential store
(Keychain, Credential Manager or Secret Service). When the index is created,
ncrs shows a recovery key once. You need it if the credential store loses the
key, for example on a new machine or after a reinstall. Without it the index
cannot be read again.

**Not encrypted:**

- The filed documents. PDFs and images stay as they are, in their folders.
- The config file.
- An index you chose to create unencrypted. ncrs asks at the first start of the
  module; "encrypted" is preselected.

**Why the index matters:** it holds the full text of all your documents in one
file. Keep that in mind for backups and cloud sync.

**Encrypted drives:** ncrs does not detect whether a folder lies on an encrypted
drive, such as a mounted Cryptomator vault. Add folders from such a drive only
to an encrypted index. With an unencrypted index, their text would end up in
plain form in a file outside the vault.

**Warnings at start:**

- *Recovery key needed.* The index is encrypted, but the credential store has no
  key for it. Enter the recovery key, and ncrs saves the key again.
- *Stored key does not open the index.* ncrs replaces the stored key only after
  you confirm it and the recovery key you enter has opened the index.
- *Index file is unencrypted although a key is stored.* The file may have been
  swapped for an unencrypted one. Nothing is taken in until you confirm. The
  confirmation lasts for the running session, and the stored key is never
  removed.

## Graphics backend

The renderer is chosen by cargo features, not in code — `src/main.rs` contains no
renderer reference at all, which makes it easy to change by accident. Three
configurations:

| Build | Backend | Use it when |
|---|---|---|
| `cargo run` (default, `gpu-with-fallback`) | wgpu, falling back to tiny-skia | **Default and releases.** Fast where a GPU exists, still starts where it does not. |
| `--no-default-features --features gpu-rendering` | wgpu, GPU | Desktop only. Needs Vulkan/Metal/DX11, no software fallback. |
| `--no-default-features --features software-rendering` | tiny-skia, software | Machines without a GPU, to rule the GPU out. Measured: visibly laggy on a high-resolution display. |

Measured on a high-resolution display: software rendering took about a second to move the
highlighted row; the fallback build was immediate. That is why the fallback build is the
default, and release builds pass `--features gpu-with-fallback` explicitly as well.
`--features` adds to the default set, so the single-backend builds need
`--no-default-features`.

A debug build prints the backend at startup, and `src/backend.rs` holds the test
that pins it to the build configuration.

```bash
cargo build --features gpu-with-fallback --release
```

The repository is a Cargo workspace (`ncrs` plus the crates under `crates/`); `cargo test --workspace` checks all of it.

## Tests

```bash
cargo test                                      # the default, release backend
cargo test --no-default-features --features software-rendering
```

The snapshot tests render with tiny-skia in every build, because their references are
tiny-skia images; `.cargo/config.toml` sets `ICED_TEST_BACKEND` for that.

Three layers, and they cover different things:

| Layer | What it covers | Where |
|---|---|---|
| Filesystem, state, routing, selection | what the app *decides* | `src/fs/`, `src/dialog.rs`, `src/selection.rs`, `app.rs` |
| UI, headless | what the user *sees* — clicks, rendering, layout | `src/ui_tests.rs` via `iced_test` |
| Snapshots | the images themselves | `tests/snapshots/` — **macOS only in CI** |

The snapshot tests run only where the reference images were produced. Font
rasterisation differs per operating system, so a macOS reference does not match
Windows output; the tests are skipped elsewhere rather than compared against a
reference that cannot hold.

The UI layer is [iced's own harness](https://docs.rs/iced_test): `Simulator` renders a
view headless and can click, type, tap keys and snapshot it. References in
`tests/snapshots/` are written on the first run and compared afterwards, so a layout
change shows up as a failing image.

Two limits of iced 0.14 worth knowing: `iced_selector` finds widgets by `widget::Id`, and
`Button` has no `id` method — so buttons are addressed by the position the layout reports
for them. And `Snapshot` offers no bytes, only a comparison against a file.

## Performance

One ignored test measures loading a large directory and moving through it, and
prints a Markdown table you can paste into an issue:

```bash
cargo test --release --bin ncrs perf_numbers -- --ignored --nocapture
```

It creates directories of 1,000, 10,000 and 100,000 entries (one in twenty a
directory, files up to 4 KiB), reads each through the same code a panel uses,
and drives the app with the same messages a key press sends. The key columns
are the mean of 1,000 presses, the others the median of seven runs, warm page
cache. Apple M4 Pro, 48 GB, macOS 26.6:

| Entries | Open directory | Sort by name / size / date | Hand to panel | Build view | ↓ + view | PgDn + view |
|---:|---:|---:|---:|---:|---:|---:|
| 1,000 | 1.6–3.1 ms | 33 µs / 31 µs / 29 µs | 14 µs | 34 µs | 30 µs | 29 µs |
| 10,000 | 22–28 ms | 0.33 ms / 0.42 ms / 0.31 ms | 0.14 ms | 29 µs | 29 µs | 30 µs |
| 100,000 | 258–346 ms | 3.2 ms / 6.5 ms / 3.1 ms | 1.4 ms | 28 µs | 29 µs | 29 µs |

What the columns are, and what they leave out:

- **Open directory** is `readdir`, one `stat` per entry and the sort by name, on
  tokio's blocking pool. The `stat` calls are nearly all of it; the range is
  three runs. The UI stays responsive meanwhile: the panel keeps the old
  listing, marked as loading, until the new one arrives.
- **Hand to panel** is what happens when the listing arrives: tags kept,
  cursor placed.
- **Build view** and the key columns are the widget tree `view` returns,
  rebuilt on every key press. Only the visible rows are built, so the cost
  does not grow with the directory. Rasterising that tree on the GPU or CPU
  happens afterwards in iced and is **not** in these numbers; see
  [Graphics backend](#graphics-backend) for what that costs in practice.
- A cold cache, a network drive or a slow disk change **Open directory** and
  nothing else.

## Build & run

Requirements: Rust (stable, 1.80+) via [rustup](https://rustup.rs).

```bash
cargo run            # debug build
cargo run --release  # optimized build
cargo test           # unit tests (fs reader, formatting, panel selection logic)
```

**Linux** additionally needs the usual windowing/font dev packages, e.g. on Debian/Ubuntu:

```bash
sudo apt install pkg-config libxkbcommon-dev libwayland-dev libx11-dev libfontconfig1-dev
```

macOS and Windows need no extra system packages.

## Architecture

```
src/
├── main.rs          # Entry point: iced::application builder (tokio executor)
├── app.rs           # App state, update() – the ONLY place state changes – and view()
├── selection.rs     # Tagged rows: the rule an operation applies to
├── dialog.rs        # State of the modal prompt (F7 and the operations after it)
├── messages.rs      # Central Message enum + PanelSide
├── keymap.rs        # Key press -> Message mapping, shortcut list for the header
├── ui/              # Pure, reusable view components (no business logic)
│   ├── panel.rs     # PanelState (data + selection/scroll helpers) + view()
│   ├── dialog.rs    # The prompt: text field, buttons, scrim
│   ├── header.rs    # Title bar with shortcut hints
│   ├── statusbar.rs # Active path | selected entry | size | date (or error)
│   ├── theme.rs     # Colors, spacing, font sizes, widget style functions
│   ├── layout.rs    # Fixed metrics (row height, column widths, visible rows)
│   └── format.rs    # Size / date formatting
├── i18n/            # Translated UI strings + per-string translator context
│   ├── mod.rs        # Msg enum, Language, Msg::note() (context for translators)
│   ├── lang/en.rs    # Source language
│   └── lang/de.rs    # German
└── fs/              # Filesystem layer – knows nothing about the UI
    ├── entry.rs     # FileEntry (name, path, size, modified, is_dir, ...) + sort order
    ├── ops.rs       # create_dir() and friends, structured errors, no text
    └── reader.rs    # async read_directory(), home_dir(), root_of()
```

### Data flow (Elm architecture)

```
 keyboard / mouse / window events
            │  (keymap::map_key, subscriptions)
            ▼
        Message ──► App::update(&mut self) ──► Task<Message> (async fs work)
            ▲                │                         │
            │                ▼                         │
            │          App::view(&self)                │
            │      (pure: state -> Element)            │
            └──────── Message::DirectoryLoaded ◄───────┘
```

### Principles

1. **No business logic in views.** `ui::*::view` functions take state and return
   `Element`s – nothing else.
2. **All state mutations go through messages.** `App::update` is the single place where
   state changes.
3. **Filesystem work is async.** `fs::read_directory` runs on tokio's blocking pool via
   `Task::perform`; results come back as `Message::DirectoryLoaded`.
4. **Components are functions, not stateful widgets.** `PanelState` lives in `App`;
   `panel::view` just renders it.
5. **Components are generic over the message type.** `panel::view` receives an
   `on_row_click: impl Fn(usize) -> M` callback, `header::view` / `statusbar::view` are
   generic `M` – they can be reused in any iced app or in other screens (dialogs, viewers…).
6. **Minimal exports.** `fs` re-exports only `FileEntry` and the reader functions; the UI
   module exposes components and `PanelState`/`PanelProps`.

### Localization

`strings.json` in the repository root is the **source of truth**. Per string it holds the
stable key, the English text, the German text and a `context` field: where the string
appears and what it means, written for someone working from that file alone — because
"Open" as a button label, as a menu entry and inside an error message are three
different strings.

`build.rs` generates the Rust modules from it, so text and context cannot drift apart.
It fails the build when a string has no usable context or is missing a translation.

```bash
# every string with its context and both translations, for a translator
cargo test dump -- --nocapture
```

Adding or changing a string:

1. Edit `strings.json`. `cargo build` regenerates the modules.
2. A new key becomes a `Msg` variant, so every call site that should use it is a
   compile error until updated.

Adding a language:

1. Add the language to `strings.json` and to `build.rs`'s language list.
2. Add the variant to `Language` and its lookup in `src/i18n/mod.rs`, with its config
   code in `Language::code()` and an entry in `Language::ALL`; the compiler does not check
   `ALL`, and a language missing there cannot be set with `lang`.
3. `cargo test` — placeholder mismatches and untranslated copies fail the build, not the
   UI. `<DIR>` and `<UP>` are deliberately never translated.

`<DIR>` and `<UP>` are deliberately never translated: they are a Norton Commander
convention, and the tests enforce it.

### Scrolling

Rows have a fixed height (`ui::layout::ROW_HEIGHT`). On window resize the app recomputes
`visible_rows`; each panel renders `entries[scroll_offset .. scroll_offset + visible_rows]`
and `PanelState::ensure_visible` keeps the selection on screen. This is O(visible rows)
per frame, independent of directory size.

## Extending NC-rs

### Add a keyboard shortcut / action

1. Add a variant to `Message` in `src/messages.rs`, e.g. `ToggleHidden`.
2. Map a key in `keymap::map_key` (`Key::Named(Named::F3) => Some(Message::View)`),
   optionally add it to `keymap::SHORTCUTS` so it shows up in the header.
3. Handle it in `App::update`. Keep the view untouched unless something new is displayed.

### Add a filesystem operation (copy, move, delete, mkdir …)

1. Implement it as an `async fn` in a new file under `src/fs/` (use
   `tokio::task::spawn_blocking` or `tokio::fs`), return a plain result type.
2. Re-export it from `src/fs/mod.rs`.
3. In `App::update`, start it with `Task::perform(fs::op(src, dst), |r| Message::OpDone(r))`.
   Use `active_panel()` for the source and `inactive_panel_mut()` for the target panel.
4. On completion, reload the affected panels with `App::load`.

### Add a UI component (dialog, file viewer, command line …)

1. Create `src/ui/<component>.rs` with a `pub fn view<'a, M: Clone + 'a>(…) -> Element<'a, M>`.
   Pass everything it needs as arguments (state refs, props struct, message callbacks).
2. Put its colors/styles into `ui/theme.rs` and fixed sizes into `ui/layout.rs`.
3. Keep its state (if any) as a field in `App` (e.g. `dialog: Option<DialogState>`) and
   compose it in `App::view` (e.g. with `iced::widget::stack` for overlays).

### Add a column (permissions, extension …)

1. Add the field to `FileEntry` and fill it in `FileEntry::from_dir_entry`.
2. Add a formatter to `ui/format.rs`, a width to `ui/layout.rs`, and the cell to
   `panel::column_header` / `panel::file_row`.

### Ideas for next steps

- Multi-selection (`Insert`), quick search by typing
- Filesystem watching (`notify` crate) as an iced `Subscription`
- Configurable theme (in the existing config file)

## Tech notes

- Font: the built-in monospace font for an authentic commander look.
- The `advanced` iced feature is **not** enabled: the app uses no `pane_grid`, `tooltip`,
  `pick_list` or `text::Font` variant. Add it back in `Cargo.toml` when you reach for one
  of those.
- The compiler is pinned in `rust-toolchain.toml`, and that pin only takes effect
  under **rustup**. A Homebrew-installed Rust ignores the file, so a local
  `cargo test` can run a different compiler than CI. `thiserror` and `softbuffer`,
  both pulled in by iced, have each broken the build on a brand-new stable.

## Trademark

Norton Commander is a trademark of its respective owner. ncrs is not affiliated
with or endorsed by it.

## License

MIT

Release archives and the macOS app include `THIRD_PARTY_LICENSES.html` with the
licenses of the dependencies and of the embedded Fira Sans font. To generate it
locally: `cargo install cargo-about --locked --features cli && cargo about generate about.hbs -o THIRD_PARTY_LICENSES.html`

## Contributing, security, changes

- [CONTRIBUTING.md](CONTRIBUTING.md) — the rules the code follows, and how to
  verify a change
- [CHANGELOG.md](CHANGELOG.md) — what each release does, and what it does not
- [SECURITY.md](SECURITY.md) — how to report a vulnerability privately
- [ARCHITECTURE_REVIEW.md](ARCHITECTURE_REVIEW.md) — the state of the
  architecture, the numbered findings, and the order they are being fixed in
