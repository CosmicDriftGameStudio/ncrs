# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Nothing yet.

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
