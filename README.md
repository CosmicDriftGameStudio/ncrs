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

## Install

| System | Command |
| --- | --- |
| macOS | `brew install --cask cosmicdriftgamestudio/ncrs/ncrs` |
| Debian, Ubuntu | `sudo apt install ./ncrs-x86_64-unknown-linux-gnu.deb` |
| Fedora | `sudo dnf install ./ncrs-x86_64-unknown-linux-gnu.rpm` |
| openSUSE | `sudo zypper install ./ncrs-x86_64-unknown-linux-gnu.rpm` |

On Windows, and without a package manager on Linux and macOS, use the install
script:

```bash
curl -fsSL https://raw.githubusercontent.com/CosmicDriftGameStudio/ncrs/main/install.sh | sh
```

On Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/CosmicDriftGameStudio/ncrs/main/install.ps1 | iex
```

The `.deb` and `.rpm` files are attached to each [release](../../releases/latest);
download the one for your CPU (`x86_64` or `aarch64`) first. They pull in the
libraries ncrs needs. The Linux binaries, packaged or
not, need glibc 2.39 or newer: Ubuntu 24.04, Debian 13, Fedora 40, openSUSE
Tumbleweed, Arch Linux and later. On an older distribution, build ncrs from
source (see [Build & run](#build--run)).

On macOS, Homebrew installs `ncrs.app` into Applications (Launchpad, Dock,
Spotlight) and puts `ncrs` on your `PATH`. The cask lives in the tap
[`CosmicDriftGameStudio/homebrew-ncrs`](https://github.com/CosmicDriftGameStudio/homebrew-ncrs);
it is not in homebrew-cask upstream. Started from Finder, the Dock or
Launchpad, the left panel opens in `$HOME` instead of `/`. From a terminal it
still opens in the working directory.

`install.sh` installs into `~/.local/bin`, verifies the download checksum,
needs no `sudo`, and is safe to re-run. Remove it again with:

```bash
curl -fsSL https://raw.githubusercontent.com/CosmicDriftGameStudio/ncrs/main/install.sh | sh -s -- --uninstall
```

Set `NCRS_BIN_DIR` to install elsewhere, or `NCRS_REPO` if you fork it.

Every package, archive and the app contain the PDF library (pdfium) next to the
binary, so ncrs reads PDFs without anything else installed. `install.sh` and
`install.ps1` put it beside the installed binary.

## Screenshots

![The drive menu on Alt+F1](docs/screenshots/drive-menu.png)

The drive menu, called Planets in the app (`Alt+F1` for the left panel, `Alt+F2` for the right).
Below the drives it lists your favourite folders: add the active folder with
`Ctrl`+`D` (`Cmd`+`D` on macOS), remove one with `Del` (`⌫` on macOS) in the menu. A favourite whose folder is gone
stays in the list, dimmed, and says so when you pick it.

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
- Confirmations (e.g. *Path copied*) appear at the right of the status bar for three seconds
- Mouse click selects a row and activates its panel; the wheel and the trackpad scroll the panel under the pointer
- Planets (drive menu, `Alt`+`F1` / `Alt`+`F2`): jump a panel to the system volume, home, iCloud Drive and
  cloud storage folders such as Google Drive, OneDrive or Dropbox (macOS), mounted volumes or
  drive letters, the same places as in the Finder sidebar; see [Drives on macOS](#drives-on-macos)
- Smell-o-scope (search) in file names and contents below a directory, in the panel, the way
  VS Code does it: see [Search](#search)
- Archives (zip, tar, tar.gz, tar.zst, 7z) open with `Enter` and are browsed like folders; `F5`
  copies out of them. Read-only for now; entries with `..` or absolute paths are left out
- SFTP servers open like folders (password login): "Connect to server…" in the command
  list, a saved server in the drive menu, or `sftp://user@host/path` in the path field.
  Browse, view (`F3`) and copy (`F5`) in both directions; see [SFTP servers](#sftp-servers)
- Localized UI (English/German) with a translator note on every string, so the UI can
  be translated mechanically without reading the code
- Runs on Linux, macOS and Windows

## Keyboard shortcuts

The defaults. Most of them can be changed in the keyboard shortcuts dialog (`Cmd`+`K`,
`Ctrl`+`K` on Linux and Windows) or in the config file, see [Keys](#keys).

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
| `F2`               | Focus the command line below the panels: `cd` changes the active panel's folder, any other command runs in the shell in that folder (the status bar shows its exit status; its output is not shown). `↑`/`↓` walk the commands of this session, `Esc` or `F2` goes back to the panels |
| `F3` / `F4`        | View / edit the file under the cursor in an external program (programs configurable, see [Configuration](#configuration)) |
| `F5` / `F6`        | Copy / move to the other panel          |
| `F7`               | Create directory                        |
| `Shift`+`F4`       | New file: creates an empty file under the name asked for and opens it in the editor like `F4`; an existing name is never overwritten |
| `F8`               | Delete to the trash, after a confirmation |
| `Shift`+`F8`       | Delete permanently (`Enter` cancels; `Shift`+`F8` again confirms) |
| `Alt`+`F1` / `Alt`+`F2` | Planets (drive menu) for the left / right panel (`↑`/`↓`, `Home`/`End`, `Enter` goes, `Esc` closes; `Option` on macOS); below the drives come the favourite folders (`Enter` goes, `Del` (`⌫` on macOS) removes) |
| `Alt`+`←` / `Alt`+`→` (also `Cmd`+`[` / `Cmd`+`]` on macOS) | Back / forward in the active panel's folder history |
| `Ctrl`+`D` (`Cmd`+`D` on macOS) | Add the active panel's folder to the favourites (`remove_favourite` and `favourites` have no default key) |
| Right click / `Shift`+`F10` / `Option` tapped alone | Context menu at the pointer or the cursor row (`↑`/`↓`, `Home`/`End`, `Enter` runs, `Esc` closes; `Ctrl`+click also on a Mac) |
| Click a column title | Sort that panel by Name / Size / Modified; a second click reverses the order (▲ / ▼). `..` and folders stay on top |
| `Ctrl`+`F3` / `Ctrl`+`F5` / `Ctrl`+`F6` (macOS: `Cmd`+`1` / `Cmd`+`3` / `Cmd`+`2`) | Sort the active panel by name / modification time / size; again reverses. macOS uses `Cmd`+digit because the system takes `Ctrl`+`F`-keys |
| `Alt`+`F7` / `Cmd`+`Shift`+`F` (`Ctrl`+`Shift`+`F` on Linux and Windows) | Smell-o-scope (search) below the directory of the active panel; pressed again, it moves to the next field. See [Search](#search) |
| `F1` / `Cmd`+`Shift`+`P` / `Ctrl`+`Shift`+`P` | Lab (command palette): lists the actions that work where you are, with their keys; type to filter, `↑`/`↓` move, `Enter` runs, `Esc` closes |
| `Cmd`+`P` / `Ctrl`+`P` | Dark Matter Drive (Quick Open): jump to a file by a few letters of its path. See [Quick Open](#quick-open) |
| `Cmd`+`Shift`+`D` / `Ctrl`+`Shift`+`D`, or click the filing progress in the status bar | Inspection, the filing status page (filing module on, Hermes): pipeline state, load, workers, index figures and the last errors. `P` pauses or resumes, `1`/`2`/`3` set the load gentle ("By the book") / balanced ("Business as usual") / full ("Grade 36") for this session, `+`/`-` change the worker count, `↑`/`↓` and `Enter` show the file of an error, `Esc` closes |
| `Cmd`+`Shift`+`L` / `Ctrl`+`Shift`+`L` | Hermes setup: choose the Tubes and document folders and save them to the config file. See [Filing](#filing). `Esc` closes |
| `Backspace`        | Go to parent directory                   |
| `Tab`              | Switch active panel                      |
| `F9`               | Switch language for this session (start: `lang`) |
| `Cmd`+`K` / `Ctrl`+`K` | Keyboard shortcuts: every action with its keys. Type to search by name, or press a key to find what it does. `Enter` records a new key for the selected action, `←`/`→` pick one of its keys or the slot that adds one, `Del` (`⌫` on a Mac) removes the key, `Shift`+`Del` resets the action. A key another action holds is only taken after you confirm. Changes apply at once and are written to `[keys]` |
| `F10` / `Cmd`+`Q` / `Ctrl`+`Q` | Quit                                     |

On macOS every command is also in the menu bar (File, Edit, View, Go, Search, Filing, Window,
Help), with the keys from the keymap. Only keys that cannot be typed into a text field are shown
there: those with `Cmd` or `Ctrl`, and the function keys. While a dialog is open everything in the menu except Quit is off, so
the keys reach the dialog. On Linux, and for now on Windows, the Lab (`F1`) is the menu.

## Search

The Smell-o-scope: `Alt`+`F7` or `Cmd`+`Shift`+`F` (`Ctrl`+`Shift`+`F` on Linux and Windows) opens
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

The Dark Matter Drive: `Cmd`+`P` (`Ctrl`+`P` on Linux and Windows) opens a box over the panels, as in
VS Code. Type letters of a file's path and the best matches are listed with the
matching characters highlighted: `cfgrs` finds `src/config.rs`. It lists the
files below the active panel's directory, hidden ones too and without following
links, up to 250,000 files (the status line says when it stopped there).
`↑`/`↓` pick a hit, `Enter` (or a click) shows its directory in the active panel
with the cursor on the file, and `Esc` closes the box.

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

## SFTP servers

A server opens in a panel like any folder. Connect with "Connect to server…" (command
list, no key by default), pick a saved server in the drive menu (`Alt`+`F1`/`F2`), or
type `sftp://user@host[:port]/path` into the path field. Login is by password only: no
key files and no ssh-agent.

- **Host keys.** The first time ncrs meets a server it shows the SHA256 fingerprint and
  asks before it sends the password. Confirmed keys go to `known_hosts` next to the
  index, never to `~/.ssh/known_hosts`, which ncrs only reads. The server is asked for
  the key type on record first. A key that differs from a recorded one, a key of
  another type than the one on record for that host and port, and a key marked
  `@revoked` are refused without a way to trust them. A `known_hosts` file that exists
  but cannot be read (permissions; in the App Store version, before the home folder is
  allowed) leaves an unknown server unchecked, and ncrs does not offer to trust it.
  Servers that offer only RSA host keys, and servers that take no password (key or
  keyboard-interactive login only), are not supported yet.
- **Passwords.** "Remember password in the keychain" keeps it in the system credential
  store, one entry per `user@host:port` (service `ncrs-sftp`, `ncrs-appstore-sftp` in the
  App Store build, which never touches the entries of an installed ncrs). It is never
  written to a file, and `sftp://user:password@host` is refused. A remembered password
  the server no longer accepts is removed from the keychain.
- **What works.** Browsing, `F3` (a temporary copy, removed when the panel leaves the
  server; on macOS and Linux readable by you only) and `F5` in both directions through
  the copy queue and the conflict dialog. Links are shown, but not copied: `F3` or `F5`
  on a link on the server (or a local link sent to a server) fails with a note, and links
  inside a folder are left out. Names with control or bidirectional characters are not
  listed and are refused when a copy meets one. A copy is written under a hidden
  `.ncrs-partial-…` name and renamed when complete; when the connection breaks during an
  upload, such a leftover can stay on the server and is safe to delete. Everything that would change something on a server, the command
  line, search and Quick Open say so instead of running. Not supported yet: moving,
  copying between servers, counting ahead, resuming, public-key login, `ssh_config`.
- **Saved servers** are `[[servers]]` entries in `config.toml` (or the Servers page of
  the settings tab, which also forgets the password of a removed one):

```toml
[[servers]]
name = "Work"          # optional; shown in the drive menu
host = "files.example.org"
port = 2222            # optional, default 22
user = "marc"
path = "/srv/files"    # optional start folder, absolute
```

A server is never written to `state.toml`: after a restart its panel is in the home
folder.

## Configuration

ncrs reads an optional TOML file. Every value in it can also be set in the
settings tab (`Cmd`+`,`, `Ctrl`+`,` on Linux and Windows): appearance and
colors, language, the viewer and editor, keyboard shortcuts and Hermes with
its AI provider. The tab writes only the changed lines and keeps comments and
order, refuses a value that would not load and shows why at its row. A file
with an error still opens the tab, which names the line and saves nothing
until it is fixed.

| System | Location |
|---|---|
| macOS | `~/.config/ncrs/config.toml` (`XDG_CONFIG_HOME` is ignored, so a start from the Dock and from a terminal read the same file) |
| Linux and other Unix | `$XDG_CONFIG_HOME/ncrs/config.toml` if set and absolute, else `~/.config/ncrs/config.toml` |
| Windows | `%APPDATA%\ncrs\config.toml` |

```toml
version = 1
# Start language: "en" or "de". F9 switches it for the running session only;
# the settings tab writes it to this file.
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

`[keys]` rebinds actions. The keyboard shortcuts dialog (`Cmd`+`K`, `Ctrl`+`K`
on Linux and Windows) writes this section for you, only the lines of the actions
you changed, and keeps the rest of the file and its comments. Each entry names an
action and gives one key or a list of keys:

```toml
[keys]
# F12 copies, F5 does nothing any more.
copy = "F12"
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
  be fine on a Mac and conflict elsewhere. Keys that became defaults later give
  way to a binding of your own instead: `F2` (`command_line`), `Shift+F4`
  (`new_file`), the history keys (`history_back`, `history_forward`),
  `primary+d` (`add_favourite`) and `primary+k` (`key_bindings`). The action keeps its other keys and the
  command list.
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
`copy`, `move`, `mkdir`, `new_file`, `delete`, `delete_permanently`, `abort_job`,
`copy_path`, `copy_name`, `drive_menu_left`, `drive_menu_right`,
`history_back`, `history_forward`, `add_favourite`, `remove_favourite`, `favourites`,
`sort_by_name`, `sort_by_modified`, `sort_by_size`, `context_menu`, `search`, `quick_open`, `command_line`, `command_palette`, `search_case`,
`search_whole_word`, `search_regex`, `filing_status`, `filing_setup`, `key_bindings`. Their
default keys are in [Keyboard shortcuts](#keyboard-shortcuts).

### Colors

`[theme]` picks a ready-made palette and changes single colors on top of it.
There are four: `cosmic` is the default, a calm, almost neutral dark grey with
muted accents; `default` is the earlier dark teal look; `classic` is Norton
Commander blue; `dracula` is the Dracula look.

The `dracula` preset uses the colors of the
[Dracula theme](https://draculatheme.com), MIT License, Copyright (c)
Dracula Theme.

```toml
[theme]
# "cosmic" (the default), "default" (the earlier dark teal look), "classic"
# (Norton Commander blue) or "dracula".
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
| `bar` | The title of the active panel |
| `bar_inactive` | Title and frame of the inactive panel |
| `status_bar` | The status bar |
| `dialog` | Dialogs |
| `menu` | The context menu |
| `accent` | Column titles, the path, dialog titles, tag stars, menu shortcuts |
| `success` | Confirmations in the status bar |
| `error` | Errors |

- Roles not listed keep the preset's colors; without `[theme]` the `cosmic` preset applies.
  To keep the earlier look, write `[theme]` with `preset = "default"`.
  The order in the file does not matter, `preset` always applies first.
- An unknown role or preset and a color in another form (names, `#rrggbbaa`) are
  errors with the line, handled like any other config error.
- A color that makes text hard to read (text on `panel`, `status_bar`, `dialog`
  or `menu`, `error` and `success` on `status_bar`, directory names on `panel`,
  `cursor_text` on `cursor`, below 3:1) is
  applied anyway; the status bar says which pair and where, until `Esc`.
- Colors are read at start; restart ncrs after editing the file.

#### VS Code themes

`vscode` takes the colors from a VS Code color theme file. The installed
themes are JSON files at `~/.vscode/extensions/<publisher>.<name>-<version>/themes/*.json`,
on Windows at `%USERPROFILE%\.vscode\extensions\...`.

```toml
[theme]
vscode = "themes/my-theme.json"
# Single roles still win over the theme file.
cursor = "#335577"
```

- The path is relative to the folder of the config file, or starts with `~/`.
- The order is: the preset, then the theme file, then the roles written in
  `[theme]`. A role the file does not define keeps the preset's color.
- Only the `colors` object of the file counts, `tokenColors` and the rest are
  ignored. Comments and trailing commas are fine. Colors are `#rgb`, `#rgba`,
  `#rrggbb` or `#rrggbbaa`; a translucent color is mixed onto the color below
  it (the panel, or the cursor row for `cursor_text`). Any other value is
  skipped.
- `include` (a path relative to the theme file) reads one more file whose
  colors the including file overrides. The `include` of that file is not
  followed.
- If the file is missing, too big (over 1 MB), not valid JSON, has no `colors`
  object or includes itself, ncrs starts anyway with the preset and your roles,
  and the status bar names the file and the reason, until `Esc`.

For each role the first key present counts:

| Role | VS Code keys |
|---|---|
| `background` | `sideBar.background`, `editorGroupHeader.tabsBackground`, `editor.background` |
| `panel` | `editor.background` |
| `text` | `editor.foreground`, `foreground` |
| `dim_text` | `descriptionForeground`, `editorLineNumber.foreground` |
| `directory` | `terminal.ansiBrightWhite`, `editor.foreground` |
| `cursor` | `list.activeSelectionBackground` |
| `cursor_text` | `list.activeSelectionForeground` |
| `tagged` | `terminal.ansiMagenta`, `list.highlightForeground` |
| `border` | `focusBorder` |
| `bar` | `titleBar.activeBackground` |
| `bar_inactive` | `titleBar.inactiveBackground` |
| `status_bar` | `statusBar.background` |
| `dialog` | `editorWidget.background` |
| `menu` | `menu.background`, `editorWidget.background` |
| `accent` | `terminal.ansiYellow`, `textLink.foreground` |
| `success` | `terminal.ansiGreen` |
| `error` | `errorForeground`, `terminal.ansiRed` |

The text on the title bars is `titleBar.activeForeground`; without it, and when
the theme sets `bar`, it is `text` or `background`, whichever reads better on
`bar`. The text on tagged rows is chosen the same way when the theme sets `tagged`.

##### Fetch a theme from Open VSX

The Lab (command palette) has the entry "Fetch color theme…". It downloads a
color theme from [Open VSX](https://open-vsx.org) and switches it on at once.

- Type an extension ID such as `dracula-theme.theme-dracula`, a Marketplace
  link (`https://marketplace.visualstudio.com/items?itemName=<publisher>.<name>`)
  or an Open VSX link (`https://open-vsx.org/extension/<publisher>/<name>`), and
  press Enter. An extension with several themes asks which one.
- This is the only place where ncrs goes onto the network, and only after you
  press Enter or Fetch. It talks to `open-vsx.org` (HTTPS) and to the download
  host Open VSX redirects to, `openvsx.eclipsecontent.org`, and to nothing else.
- The files go to `themes/<publisher>.<name>/` next to the config file, together
  with the extension's license file, and `vscode` in `[theme]` points at the
  theme. The rest of the config file stays as written.
- The license named by Open VSX is shown while you choose and in the status bar
  after the installation. Check that it allows what you do with the theme.

### Filing

`[modules.filing]` switches the filing module on and lists the folders it works
on:

```toml
[modules.filing]
enabled = true
# "gentle", "balanced", "full" or a number of workers.
load = "balanced"

[[modules.filing.locations]]
path = "~/Inbox"
mode = "inbox"
images = false

[[modules.filing.locations]]
path = "~/Documents/Archive"
mode = "archive"
images = true
target = true
```

- An `inbox` is emptied into the target. An `archive` is indexed where it is.
  `images` also takes in images, not only PDFs.
- Exactly one archive is the `target`, the archive filed documents go to. An
  inbox cannot be the target, and no location may be the same folder as another
  or lie inside it.
- `load` has the names `gentle`, `balanced` and `full`; the Inspection shows
  them as "By the book", "Business as usual" and "Grade 36" (full load).

The filing module is called Hermes in the app. In the setup and on the
Inspection the config terms read like this:

| Config | English | German |
| --- | --- | --- |
| `mode = "inbox"` | Tubes: where new documents arrive, emptied | Rohrpost |
| `mode = "archive"` and `target = true` | Central Filing: documents are filed here | Zentralregistratur |
| `mode = "archive"` without `target` | Records: only read, nothing is moved | Aktenkeller |
| the filing status page | Inspection | Inspektion |
| the review list | Pending | Wiedervorlage |

**Setup.** `Cmd`+`Shift`+`L` (`Ctrl`+`Shift`+`L` on Linux and Windows,
`filing_setup` in `[keys]`) opens the setup, also from the command palette
(`F1`) as "Hermes: choose folders". It also opens on its own when the module is
on and no locations are listed. The card has two sections: Tubes, where new
documents arrive (optional), and the folders where you keep your documents (at
least one). "Add folder…" opens the system folder dialog; "Use the folder in
the panel" adds the folder of the active panel instead, for keyboard use and
for systems without a dialog. The card stays open and the panels stay usable.
Each entry has its images switch and can be removed. A folder that is already
in the list is not added again; the card says "already added".

With one document folder, it is the Central Filing. From two document folders
on, pick the Central Filing with the radio buttons; saving waits for the
choice. A list that breaks a rule, for example a Tubes folder inside a document
folder, says so and cannot be saved.

Saving writes the locations by their real paths (symlinks resolved) to
`[modules.filing]` and sets `enabled = true`. The rest of the file, comments
included, stays as it is. A config file that does not load is not touched: fix
it first, the setup shows the error. After saving, the card closes and the
status bar says that Hermes is on and which key opens the Inspection. If no
filing runs on configured locations yet, filing starts at once with the saved
ones; otherwise the status bar says they apply from the next start of ncrs.

**AI.** Documents the rules cannot read can be handed to a small language
model. It is off until switched on:

```toml
[modules.filing.ai]
enabled = true
# "local" (default) runs a model on this machine; "http" talks to a server.
provider = "local"
# local only: threads for the model, 1 or more; left out, ncrs picks.
threads = 4

# provider = "http" needs both:
# endpoint = "http://localhost:8080"
# model = "qwen3"
```

- `local` uses Qwen3 0.6B (Apache-2.0), one file of about 610 MB that is not
  part of ncrs. When it is missing, ncrs asks once per start whether to download
  it; the card shows host, file, size, checksum and folder first, and nothing
  is fetched before you press the button. The command palette entry "Download
  AI model…" (`download_ai_model` in `[keys]`, no key by default) opens the card
  at any time. An interrupted download continues where it stopped, and the file
  is checked against its size and checksum before it is used.
- `http` sends text to the server at `endpoint`, which must be on this machine
  for now (`localhost`, `127.0.0.1` or `::1`); `endpoint` and `model` are
  required and `threads` is not allowed. Without a consent given for that
  server, nothing is sent.
- The Inspection shows a row "AI model" with the state while the AI is on.

### Remembered state

ncrs remembers both panels' folders, sort and cursor row, the active panel and
the window size in `state.toml`, next to `index.db`
(`~/Library/Application Support/ncrs` on macOS, `$XDG_DATA_HOME/ncrs` or `~/.local/share/ncrs` on Linux,
`%APPDATA%\ncrs` on Windows, `data` in portable mode). ncrs writes it when it
quits and shortly after a change; `config.toml` is never written for this. A
folder that is gone opens its nearest existing parent (with the cursor on the
first row); so does a cursor entry that is gone. Each panel's back and
forward history (50 steps) and the favourites are kept too. A missing or broken
file means the defaults, and you can delete it to start fresh.

### Portable mode

If a folder named `data` sits next to the ncrs executable, ncrs is portable:
`data/config.toml`, `data/index.db` and `data/state.toml` are used instead of
the locations above. Inside a macOS `.app` bundle the folder is
ignored. The Windows release has two ZIPs: `ncrs-x86_64-pc-windows-msvc.zip` uses
`%APPDATA%\ncrs`, `ncrs-x86_64-pc-windows-msvc-portable.zip` ships with the
`data` folder. Delete the folder to use `%APPDATA%\ncrs` again.

The key of an encrypted index stays in the operating system's credential store
(on Windows the Credential Manager), under an entry of its own per data folder,
so portable copies and an installed ncrs do not overwrite each other's key. The
entry is named by a random ID in `data/key-id`, which ncrs creates when it first
stores a key. Moving the folder or a different drive letter changes nothing; on
another computer the index needs the recovery key. Copies of the folder share
the entry. If `key-id` gets damaged, delete the file; ncrs then asks for the
recovery key.

To make an existing installation portable, copy `%APPDATA%\ncrs\config.toml`
to `data\`.

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
ncrs shows your master key once, the same key ncrs later asks for as the
recovery key. You need it if the credential store loses the
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
sudo apt install pkg-config libxkbcommon-dev libwayland-dev libx11-dev libfontconfig1-dev cmake libclang-dev
```

The default build compiles llama.cpp for the built-in AI model, so it needs
[cmake](https://cmake.org), a C++17 compiler and clang (libclang, for the
bindings) on every system: Xcode's command line tools and `brew install cmake` on
macOS, Visual Studio's C++ tools, LLVM and cmake on Windows. A build without the
model needs none of them: `cargo build --no-default-features --features gpu-with-fallback`.

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
   `on_row_click: impl Fn(usize) -> M` callback, and `statusbar::view` is
   generic over `M` – they can be reused in any iced app or in other screens (dialogs, viewers…).
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
licenses of the dependencies, the embedded Fira Sans font and the Lucide icons. The About dialog
(macOS app menu, or "About ncrs" in the `F1` command list) opens it. To generate it
locally: `cargo install cargo-about --locked --features cli && cargo about generate about.hbs -o THIRD_PARTY_LICENSES.html`

No dependency may be under GPL, LGPL or AGPL, because those licenses rule out
the Mac App Store; `deny.toml` explains how `cargo deny` enforces it.

| Shipped or fetched | License | Where its text is |
| --- | --- | --- |
| pdfium, the PDF library next to the binary ([pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) build) | BSD-3-Clause; the build itself MIT; bundled components such as FreeType, ICU, libjpeg-turbo and zlib under their own permissive licenses | `licenses/pdfium/` in the archive, `Contents/Resources/licenses/pdfium/` in the macOS app |
| OCR models (`text-detection.onnx`, `text-recognition.onnx`), downloaded only after asking | CC-BY-SA-4.0 ([ocrs-models](https://github.com/robertknight/ocrs-models), trained on HierText) | not shipped; fetched from the ocrs project |
| Local AI model `Qwen3-0.6B-Q8_0.gguf`, downloaded only after asking | Apache-2.0 ([Qwen/Qwen3-0.6B-GGUF](https://huggingface.co/Qwen/Qwen3-0.6B-GGUF)) | not shipped; fetched from Hugging Face |

## Contributing, security, changes

- [CONTRIBUTING.md](CONTRIBUTING.md) — the rules the code follows, and how to
  verify a change
- [CHANGELOG.md](CHANGELOG.md) — what each release does, and what it does not
- [SECURITY.md](SECURITY.md) — how to report a vulnerability privately
- [ARCHITECTURE_REVIEW.md](ARCHITECTURE_REVIEW.md) — the state of the
  architecture, the numbered findings, and the order they are being fixed in
