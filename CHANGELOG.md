# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `vscode = "<path>"` in `[theme]` takes the colors of a VS Code color theme
  file, under the roles written in the config. If the file cannot be used,
  ncrs starts with the preset and says why in the status bar.
- A `dracula` color preset (`preset = "dracula"` in `[theme]`), using the colors
  of the [Dracula theme](https://draculatheme.com), MIT License, Copyright (c)
  Dracula Theme.
- Playful names, each with its plain description next to it: the command
  palette is the Lab, Quick Open the Dark Matter Drive, the search the
  Smell-o-scope, the drive menu Planets, the file exists dialog a Time paradox,
  tagging stamps, and the trash dialog says "Off to the garbage ball". The
  function key bar, the context menu and the config names are unchanged.
- The command palette also finds an entry by its old name or its config name,
  e.g. "quick open" or "drive menu".
- A separate portable Windows ZIP (`ncrs-x86_64-pc-windows-msvc-portable.zip`)
  with a `data` folder. A `data` folder next to the executable holds
  `config.toml` and `index.db` instead of the usual locations, everywhere except
  in a macOS `.app` bundle. The index key of a portable copy is stored per data
  folder in the credential store, through an ID in `data/key-id`, so moving the
  folder keeps it.
- `F1` opens the command palette, as in VS Code; the function key bar shows
  it as "Commands".
- The filing setup asks in plain words: one section for the Tubes folders
  where new documents arrive and one for the folders where you keep your
  documents. "Add folder…" opens the system folder dialog, a folder already in
  the list is not added twice, and from two document folders on you choose the
  Central Filing. After saving, the card closes and the status bar says how to
  open the Inspection.

### Changed

- The NC-rs bar at the top is gone (the window title names the app);
  confirmations show at the right of the status bar. In `classic` they are
  bright green, and a `[theme]` whose `success` or `error` is hard to read on
  the status bar is reported.
- The card with the master key of a new encrypted index is titled "Your master
  key" and points at the highlighted block you type to confirm you saved it.
- The Inspection labels what waits as "Pending", names its steps Intake and
  Pending, and shows "after the first full minute" for throughput and time
  left until a full minute has been measured.
- The tag star stays readable on tagged rows in every color preset and with
  your own `[theme]` colors; the `cosmic` and `dracula` presets keep the text
  on tagged rows and on the title bars readable when `tagged` or `bar` change.
- The `cosmic` color preset is the new default, a calm, almost neutral dark
  grey with muted accents. To keep the earlier look, write `[theme]` with
  `preset = "default"` in the config.
- The filing module is called Hermes in the app: Tubes (inbox), Central Filing
  (target archive), Records (other archives), the Inspection (status page),
  Pending (review list) and the load levels "By the book", "Business as usual"
  and "Grade 36" come with a plain explanation next to them. The config keys
  and values stay as they are.
- A `[keys]` section that binds `F1` to another action now reports that `F1`
  belongs to `command_palette`. Give it other keys, e.g.
  `command_palette = ["primary+shift+p"]`, to keep your binding.

## [0.3.1] — 2026-10-22

### Added

- Release archives and the macOS app include `THIRD_PARTY_LICENSES.html`: the
  licenses and copyright notices of every dependency built into ncrs, and of
  the embedded Fira Sans font.

## [0.3.0] — 2026-10-17

### Added

- Quick Open: `Cmd`+`P` (`Ctrl`+`P` on Linux and Windows, `quick_open` in
  `[keys]`) opens a box in which a few letters of a path find a file below the
  active panel's directory (`cfgrs` finds `src/config.rs`), hidden files
  included, up to 250,000 files. `↑`/`↓` pick a hit, `Enter` shows it in the
  active panel with the cursor on it, `Esc` closes the box.
- Search in the panel, the way VS Code does it: `Alt`+`F7` or `Cmd`+`Shift`+`F`
  (`Ctrl`+`Shift`+`F` on Linux and Windows, `search` in `[keys]`) replaces the
  active panel with a search below its directory. It runs as you type, and a new
  input stops the search still running. **Aa**, **ab** and **.\*** switch on
  case, whole words and regular expressions; **Files** and **Exclude** take
  `;`-separated patterns, and **Files** without a search text is a name search.
  Results are grouped by file and can be folded, with the line number and the
  match highlighted; an invalid pattern or expression is shown in place.
  `Enter` on a file shows it in the other panel, `Enter` on a line opens the
  editor at that line. Files are tagged as in the panel (`Insert`, `Space`,
  `*`); `F5`, `F6` and `F8` act on the tagged files, else on the file under the
  cursor, and the function key bar shows only what works there. `Alt`+`C`,
  `Alt`+`W` and `Alt`+`R` (`search_case`, `search_whole_word`, `search_regex`)
  switch the three options from the keyboard. They only count inside the
  search, so a config file that gives `alt+c`, `alt+w` or `alt+r` to an action
  that only works in the panel, such as `mkdir`, keeps working. They cannot
  take the keys the search uses itself: `Enter`, the arrows, `PageUp`,
  `PageDown`, `Home` and `End`. `Esc` brings the panel back and keeps the
  fields for the next search.
- `{path}` and `{line}` in the `[open]` programs: `edit = ["code", "--goto",
  "{path}:{line}"]` opens a search result at its line. Without `{path}` the file
  is still added as the last argument.
- Start language from the config file: `lang = "en"` or `lang = "de"` at the top
  level. Without it the start language stays English. `F9` still switches the
  language for the running session only; nothing is written back to the file.
  Any other value is an error with the line that names the allowed ones, handled
  like any other config error.
- Colors from the config file: `[theme]` picks a palette with
  `preset = "default"` or `"classic"` (Norton Commander blue) and changes single
  roles on top of it as `"#rrggbb"` or `"#rgb"`:

  ```toml
  [theme]
  preset = "classic"
  directory = "#ffff55"
  tagged = "#a50"
  ```

  The roles are `background`, `panel`, `text`, `dim_text`, `directory`, `cursor`,
  `cursor_text`, `tagged`, `border`, `bar`, `bar_inactive`, `status_bar`,
  `dialog`, `menu`, `accent`, `success` and `error`. Without `[theme]` nothing
  changes. An unknown role or preset and an invalid color are errors with the
  line; a color that leaves text below 3:1 contrast is applied, and the status
  bar names the pair and the line until `Esc`.
- Keys from the config file: `[keys]` gives an action one key or a list of keys
  (`copy = "F2"`, `quit = ["F10", "primary+q"]`, `delete_permanently = []`),
  with `primary` for `Cmd` on macOS and `Ctrl` elsewhere. Actions not listed
  keep their keys. The function key bar, the context menu and the status
  messages that name a key follow the keys in effect. One key on two actions,
  an unknown action or key, `Escape`, and bare letters or digits (type-ahead)
  are errors with the line, handled like any other config error.
- A config file (TOML) at `~/.config/ncrs/config.toml` (macOS, Linux; Linux
  honours `$XDG_CONFIG_HOME`) or `%APPDATA%\ncrs\config.toml` (Windows). `[open]`
  sets the programs for `F3` (`view`) and `F4` (`edit`, also used for files that
  could run); without it the system programs apply. A missing file is fine; an
  invalid one is ignored as a whole, with the file and line shown in the status
  bar until `Esc` dismisses it.
- Confirmations such as "Path copied" now show on the right of the title bar with
  a green check mark and go away after three seconds; failures stay red in the
  status bar.
- The path in the status bar can be edited: click it, type or paste a path and
  press `Enter` (a folder opens in the active panel, a file is shown in its
  folder; `~` is the home directory, `.` and `..` are folded without following
  links; a path that is missing or unreadable is reported and the field stays
  open). `Esc`, `Tab` or a click elsewhere puts the current path back and the
  keys go to the panel again; `Cmd`+`Q` / `Ctrl`+`Q` and `F10` still quit. The
  icon next to it copies the panel's folder path.
- Type-ahead: typing a letter or digit jumps to the first entry whose name starts
  with it, ignoring case; a character typed within a second extends the prefix.
- Copy the path or the name of the selection to the clipboard: `Option`+`Cmd`+`C`
  / `Ctrl`+`Alt`+`C` for the path, `Ctrl`+`Cmd`+`C` / `Ctrl`+`Shift`+`C` for the
  name.
- A context menu on the panel rows, as in Zed: right click (`Ctrl`+click on a
  Mac), `Shift`+`F10`, or a tap of `Option` on its own. It offers the existing
  actions with their keys, acts on the tagged rows like `F5` and `F8`, and
  greys out what does not apply, for example View on a folder.
- Sort a panel by clicking Name, Size or Modified (a second click reverses it,
  ▲ / ▼ marks the column) or with `Ctrl`+`F3` / `F5` / `F6` (name / modified /
  size; on a Mac `Cmd`+`1` / `3` / `2`, as the system takes the `Ctrl` chords).
  Each panel has its own order, kept across reloads and folder changes; `..`
  and folders stay on top.

### Changed

- The context menu is darker, and its keys and greyed-out entries are easier to
  read.

## [0.2.2] — 2026-10-02

### Fixed

- The drive menu hides mounts the Finder hides (temporary disk images and
  other helper mounts), instead of listing every entry of `/Volumes`.
  macFUSE mounts name their origin, e.g. `UzeN1wWoFdWU (Cryptomator)`.
- After a delete, move or other reload of the same folder, the cursor stays
  where it was (on the next file when its own is gone) instead of jumping to
  the top of the list.

## [0.2.1] — 2026-10-02

### Added

- macOS: a notarized `ncrs.app` (`ncrs-<target>.app.zip`) with an app icon, so
  ncrs shows up in Applications, Launchpad and the Dock. The Homebrew cask
  installs it and keeps `ncrs` on the `PATH`.
- A window icon on Linux and Windows.
- Alt+F1 and Alt+F2 (Option on macOS) open a drive menu for the left or right
  panel, listing the places of the Finder sidebar: the system volume, home,
  iCloud Drive and mounted volumes (macOS), mounts under `/media`,
  `/run/media` and `/mnt` (Linux), drive letters (Windows). Arrows, Home and
  End move, Enter or a click goes there, Escape closes. Listing never touches
  a mount, so a dead network share cannot freeze it.
- The mouse wheel and the trackpad scroll the panel under the pointer. The
  cursor stays while it is in view and is held at the edge otherwise. A
  dialog or the drive menu keeps the panels behind it still.
- Cmd+Up and Cmd+Down page through the active panel (Ctrl on Linux and
  Windows), next to PageUp and PageDown.

### Changed

- Quit is `F10` or `Cmd`+`Q` (`Ctrl`+`Q` on Linux and Windows); a bare `Q` no
  longer quits and takes part in type-ahead like any other letter.
- Started from Finder (working directory `/`), the left panel opens in `$HOME`.
- ncrs is described as "inspired by Norton Commander", with a trademark notice
  in the README.

## [0.2.0] — 2026-10-02

The file operations: copy, move, delete, view and edit.

### Added

- F5 copies and F6 moves into the other panel, as a job in a queue with a
  progress line. Ctrl+C or Escape stops it. The window stays usable while it
  runs.
- When the target already exists, a dialog asks: overwrite, keep or cancel,
  optionally for all files. Overwrite builds the copy next to the target and
  swaps it in at the end, so a failed copy leaves the old target in place.
- Move renames where it can and copies only across devices. Copying a
  directory into itself, a path onto itself and special files (FIFOs,
  sockets, devices) is refused. Copies lose setuid, setgid and sticky bits;
  symlinks are copied as links.
- F8 moves to the trash. Shift+F8 deletes for good, behind a warning that
  names the entries and is confirmed only by a second Shift+F8 or a
  deliberate Enter on the delete button. A failing trash never falls back to
  a permanent delete.
- F3 views and F4 edits the file under the cursor in the system's program.
  A file that could run (exec bit, or an extension such as `.app`,
  `.command`, `.exe`, `.bat`) opens in the text editor on macOS and Windows
  and is refused on Linux.
- A Norton-style function key bar at the bottom.
- Tagging with Space and Shift+arrows, and Cmd/Ctrl-click, since Macs have
  no Insert key.
- Dialogs are centred and work from the keyboard: arrows and Tab move the
  focus, Enter presses the focused button, Escape cancels.

### Fixed

- Keys were lost whenever the app state changed, because the keyboard
  subscription was rebuilt with it.
- A held key no longer repeats actions such as F8 or a dialog answer; only
  movement, tagging and typing repeat.
- Ctrl+`*` works on layouts where `*` needs Shift.
- A space can be typed in the F7 directory name.

### Known issues

- F4 does not use `$EDITOR`: the app has no terminal. Configurable programs
  come with the configuration file.
- An aborted copy still finishes the entry it is working on.
- A move across devices loses files created in the source while it runs.

## [0.1.0] — 2026-09-29

First public release. A dual-panel file manager: navigate, select, create a
directory. No file operations yet — copying, moving and deleting are the next
milestone, and the architecture review that precedes them is in
[ARCHITECTURE_REVIEW.md](ARCHITECTURE_REVIEW.md).

### Added

- Two side-by-side panels. The left opens in the working directory, the right
  in `$HOME`, so the two are not the same place on launch.
- Keyboard navigation: arrows, PgUp/PgDn, Home/End, Tab to switch panels,
  Backspace to go up. The directory you came from is re-selected.
- Multi-selection with three distinct states, as in Norton Commander: cursor,
  tagged with Insert, and both. If anything is tagged an operation applies to
  the tagged rows, otherwise to the row under the cursor.
- Name / Size / Modified columns, directories first, case-insensitive sort,
  `..` as the first row.
- Directory reads run on tokio's blocking pool, so the UI never blocks. A
  result that no longer matches the panel it was requested for is dropped
  rather than shown.
- F7 creates a directory: dialog, background operation, reload of the panel.
  Names that cannot be created are refused while the dialog is still open.
- English and German, switched with F9. Every string carries a translator
  note in `strings.json`; `build.rs` refuses to build a string without usable
  context, so translation never becomes guesswork.
- Configurable rendering by cargo feature: `software-rendering` (no GPU
  needed), `gpu-rendering`, `gpu-with-fallback`. Releases ship the fallback
  build.
- Installers for Linux, macOS and Windows that verify a published SHA-256
  before installing.

### Fixed

- F7 started the directory creation and the panel reload at the same time. A
  reload that won the race showed a panel without the new directory, and the
  selection landed on whatever happened to be there. The reload now runs when
  the creation reports back, which also re-reads after a failure — the name
  may belong to something that appeared meanwhile.
- Directory reads cannot be cancelled, only ignored: the thread on a hanging
  network mount stayed busy. Noted in the review, still open.
- Tags were row positions rather than names, so after a reload a tag could
  point at a different file. Harmless while nothing is deleted yet; tags are
  names now, before file operations make it dangerous.

### Known issues

- No file operations yet: F5, F6 and F8 do nothing.
- Snapshot tests run on macOS only. Font rasterisation differs per platform,
  so pixel references generated on one platform do not hold on another.
- Binaries are published with a SHA-256 checksum but are not signed. The
  checksum detects a damaged download, not a compromised release.
