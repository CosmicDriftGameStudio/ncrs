# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- About dialog: the macOS app menu entry "About ncrs" and the command list
  (`F1`) show the version, the license and the repository, and open the file
  with the licenses of the third-party libraries.
- The README lists timings for 100,000 files: opening, sorting, searching by name
  and by content, copying and tagging, with the machine they were measured on
  and the known limits.
- An index from 0.6.0 or later opens and is migrated, and a failed migration
  leaves the index file as it was and says which step failed.

### Changed

- Hermes is marked as a beta in its sidebar, the settings and the command list
  (`F1`); the README says what that means.
- The config `version` can be 1 or newer: a file without it is version 1,
  older versions stay readable, and a file from a newer ncrs is refused with
  an error that names its version instead of being half understood. The state
  file of a newer ncrs is left alone: ncrs starts with the defaults and does
  not save over it. Example configs of 0.3.0, 0.5.0 and 0.7.0 are tested to
  keep loading.

## [0.8.0] — 2026-10-28

### Added

- Quick Open (`Cmd`+`P`, `Ctrl`+`P`) also finds documents by their content
  when Hermes has an index: a "Documents" section below the file name hits
  lists up to five documents, including text read from PDFs and by OCR, with
  the file name, its folder and a PDF or OCR mark. `Enter` puts the panel
  cursor on the file. The name hits stay on top and appear at once; the text
  search runs apart and never moves them. A `>` first switches to the command
  list, settings included.
- Keyboard shortcuts dialog (`Cmd`+`K`, `Ctrl`+`K` on Linux and Windows, also in
  the command list): every action with its group and keys. Search by name or by
  pressing a key; `Enter` records a new key, a second key can be added, `Del`
  removes one, `Shift`+`Del` resets the action, and "Reset all" asks first. A key
  another action holds is shown before anything is saved and only taken when
  confirmed. Changes apply at once and write only the changed `[keys]` lines,
  keeping the rest of the config and its comments. A config that already gives
  `primary+k` to another action keeps it.
- The settings tab covers the whole config, so the App Store build, whose
  `config.toml` sits in the sandbox container, needs no file editing. New pages
  Programs (viewer and editor, typed or chosen with a file dialog; the store
  build takes apps only) and Keyboard shortcuts (opens the dialog). Appearance
  gains every `[theme]` color with a swatch and the contrast warning at its
  row, and a VS Code theme file to choose or remove. Hermes gains the AI
  provider with threads, or endpoint and model name for an own server. A typed
  value is saved with `Enter`, or when the page changes or the tab closes. A
  value that would not load is shown at its row and never written, and the page
  stays; a config with an error still opens the tab and names the line.
- The command list (`Cmd`+`Shift`+`P`) also finds every settings page ("Settings:
  Programs") and every single setting ("Viewer (F3)", "Accent", "AI provider").
  Running a hit opens the settings tab on that page with the row in view and
  highlighted. They are listed under "Settings" after the commands and only
  while something is typed.
- 7z archives (`.7z`) open like the other archives, with LZMA, LZMA2, deflate
  and the BCJ filters; the same name rules and read-only limits apply. An
  encrypted 7z (content or file list) is refused with a message ("This archive
  is encrypted; ncrs cannot open it yet") instead of a read error, and so is
  one that needs more than 256 MB of memory to unpack.
- Archives open like folders: `Enter` on a `.zip`, `.tar`, `.tar.gz`/`.tgz` or
  `.tar.zst`/`.tzst` lists its content in the panel, `..` leaves it with the
  cursor on the archive. `F5` copies entries out through the copy queue and the
  conflict dialog; they are unpacked into a hidden folder in the target and
  only then renamed into place, so a cancel or a broken archive leaves nothing
  half-written. Entries with `..`, absolute paths or drive prefixes, links and
  special files are left out and counted in the status bar. Archives are
  read-only for now: `F4`, `F6`, `F7`, `Shift+F4`, `F8`, search and Quick
  Open say so instead of acting, and an archive cannot be a copy target yet. A
  panel inside an archive is remembered as the folder holding it, with the
  cursor on the archive.
- `F3` views a file inside a zip, tar or 7z archive: it is unpacked into a
  temporary copy, and the viewer opens that copy. On macOS and Linux the copy
  and its folders can only be read by you; on Windows that rests on the access
  rights of the cache folder, and in portable mode they are as open as the
  `data` folder is. The copies live in the cache folder
  (`~/Library/Caches/ncrs/view` on macOS, `$XDG_CACHE_HOME/ncrs/view` or
  `~/.cache/ncrs/view` on Linux,
  `%LOCALAPPDATA%\ncrs\view` on Windows, `data/view` in portable mode). They
  are removed once no panel shows the archive any more and when ncrs quits, and
  what a crash left behind is removed at the next start. The unpacking is a job
  in the status bar that `Esc` or `Ctrl`+`C` stops; while another job runs,
  `F3` says so and unpacks nothing. A slow viewer may find its copy gone when
  you leave the archive right away. `F4` stays refused, as an edit would be
  lost with the copy.
- SFTP servers open like folders. "Connect to server…" (command list; there is
  also "Disconnect server"), a server saved in the drive menu, or an
  `sftp://user@host[:port]/path` address in the path field connect with a
  password; no key files, no ssh-agent. The host key is checked before the
  password is sent: an unknown server shows its SHA256 fingerprint and is only
  trusted when you say so (the key goes to a `known_hosts` of ncrs's own next to
  the index, never to `~/.ssh/known_hosts`, which is only read), a changed or
  revoked key is refused. "Remember password in the keychain" keeps the password
  in the system credential store, one entry per server, never in a file.
  Browsing, `F3` (a temporary copy only you can read) and `F5` in both directions
  through the copy queue and the conflict dialog work on a server; everything
  that would change something there, the command line, search and Quick Open say
  so instead. Saved servers are `[[servers]]` entries in `config.toml` (`name`,
  `host`, `port`, `user`, `path`; no password) and a Servers page in the settings
  tab, whose "Remove" forgets the password too. Servers are never written to
  `state.toml`. Not in this version: moving to or from a server, copying between
  servers, search, Quick Open and Hermes on a server, `F4`, `F6`, `F7`, `F8`,
  rename and new file on a server, key and ssh-agent login, servers that offer
  only RSA host keys, `@cert-authority` and wildcard patterns in `known_hosts`,
  `ssh_config` and ProxyJump, resuming and parallel transfers, counting ahead and
  conflict counts for copies to or from a server, permissions and modification
  time of uploaded files, atomic replacement of a file on the server, links on a
  server (shown, but `F3` and `F5` on one fail with a note and links inside a
  folder are skipped). The copy for `F3` is readable by you only on macOS and
  Linux. After a lost connection a hidden `.ncrs-partial-…` file of an upload can
  stay on the server. A remembered password the server no longer accepts is
  removed from the keychain; the App Store build uses the keychain service
  `ncrs-appstore-sftp` and so never touches the entries of an installed ncrs.
  The server is asked for the key type on record first; a key of another type
  than the one on record is refused like a changed key, and a `known_hosts` that
  cannot be read leaves an unknown server unchecked. The App Store build needs
  the `network.client` entitlement for this, which comes with the sandbox
  build.
- Groundwork for the Mac App Store build, behind the `sandbox` cargo feature
  that no release uses yet; normal builds are unchanged. Inside the App Sandbox
  the start folder and the drive menu use the real home folder instead of the
  container, `~` in the config means the real home, folder grants are kept as
  security-scoped bookmarks and restored at every start, the index key lives
  under its own keychain service, and delete goes to the trash through the file
  manager. At the start the app asks to allow the home folder as long as it
  cannot be read; "Grant folder access…" in the command list adds more.
- `F3` and `F4` treat more launcher file types as executable: installers and
  packages (`.pkg`, `.msix`, `.deb`, `.rpm`), shortcuts and location files
  (`.webloc`, `.inetloc`, `.appref-ms`), help files (`.chm`, `.hlp`) and the
  like. They go to the editor or are refused instead of being run. A name that
  ends in dots or spaces (`evil.exe.`) is judged without them, and Windows
  refuses such names inside archives.

## [0.7.0] — 2026-10-27

### Added

- Back and forward through each panel's folder history (`Alt`+`←`/`→`, also
  `Cmd`+`[`/`]` on macOS) and favourite folders (`Ctrl`+`D`, `Cmd`+`D` on macOS,
  adds the active folder; they are listed below the drives in `Alt`+`F1`/`F2`,
  `Del`, `⌫` on macOS, removes one). History and favourites are remembered across restarts;
  rebindable as `history_back`, `history_forward`, `add_favourite`,
  `remove_favourite`, `favourites`. A config that already gives one of the new
  default keys (these and `Shift+F4`) to another action keeps it; the new
  action only loses that key.
- ncrs remembers both panels' folders, their sort, the cursor row, the active
  panel and the window size across restarts in `state.toml` next to the index.
  A folder that is gone opens its nearest existing parent; a missing or broken file means the
  defaults.
- `Shift+F4` asks for a name, creates an empty file in the active panel's folder
  and opens it in the editor like `F4`, with the cursor on it; an existing name
  is never overwritten. Rebindable as `new_file`, also in the command list.
- Command line below the panels (`F2`): `cd` changes the active panel's folder,
  any other line runs in the folder of the active panel through `$SHELL` (`cmd`
  on Windows), and the status bar shows its exit status once it ends. `↑`/`↓`
  walk the commands of this session; `Esc` or `F2` goes back to the panels.
  There is no output view yet. A config that already gives bare `F2` to another
  action keeps it; the command line then has no key and is in the command list.

### Fixed

- `THIRD_PARTY_LICENSES.html` now carries the license of the Lucide icons
  compiled into ncrs.

## [0.6.0] — 2026-10-26

### Added

- Linux packages: each release has a `.deb` (Debian, Ubuntu) and an `.rpm`
  (Fedora, openSUSE) for x86_64 and aarch64. They install the libraries ncrs
  needs and put ncrs into the application menu.

### Fixed

- PDFs were not read when ncrs was started through a symbolic link, as winget
  does: the PDF library is now looked up next to the real executable.

## [0.5.0] — 2026-10-26

### Added

- Saved queries (Inquiries): searches over your documents by year, tax
  context, kind, tags and full text, saved under a name. Running one only
  looks. Exporting copies the files it found into a folder you choose, next
  to a CSV with their fields; files with the same name get a number, and the
  CSV lists the name each was exported as. Nothing is moved, deleted or
  overwritten. Open it with Cmd+Shift+A (Ctrl+Shift+A elsewhere) or from the
  command list.
- Review list (Pending): the documents the rules could not file are listed
  with their fields, which you can correct. Confirming files the document
  with the pattern of its rule (or one you enter), journaled like every move
  and without overwriting anything; a taken place keeps it in the list. A
  confirmed document can become a rule for its sender in one step. Open it
  with Cmd+Shift+V (Ctrl+Shift+V elsewhere) or from the command list.
- Inbox documents are now filed on their own: when the rules have found a
  place for a document it is marked as planned, and a filing job moves it to
  the archive marked as the target (it also runs once when ncrs starts). The
  place is worked out again from the current rules, so a changed rule counts.
  Nothing is overwritten or deleted: if the place is taken, or no rule places
  the document any more, it goes to the review list instead, and a file that
  was edited in the meantime is taken in again. Each move is journaled, so a
  run that was cut off is settled the next time ncrs starts, and Escape stops
  the job between two files; the documents not yet moved stay planned and are
  filed by the next job.
- Reorganisation plan (Inventory): a view that shows how your document
  folders could be tidied up, without changing anything. Entries are grouped
  as duplicates, filed in the wrong folder, year does not match, name differs,
  and what cannot be done, each with its current path, the proposed path and
  the reason, plus a before and after tree of the folders the selection
  touches. You select entries one by one or a whole group; entries that rest
  on a rule you have not confirmed start unselected. Open it with Cmd+Shift+I
  (Ctrl+Shift+I elsewhere) or from the command list. Enter now carries out
  the selected entries: each file is checked against the plan, never
  overwrites anything and is never deleted, and the run can be stopped with
  Escape between two files. Every step is journaled, so "Undo last
  reorganisation" (command list, with the small name Rewind) moves the files
  back and removes the folders the run created if they are empty. A run that
  was interrupted is settled the next time the index opens. Moves across
  drives copy to a temporary name first, compare the copy with the original
  and only then remove the original.
- Rule suggestions (Requisitions): ncrs looks at how your records are
  already filed and suggests rules from it, such as "text contains Company A
  Ltd goes to Insurance". Each suggestion shows how many documents in its
  folder and elsewhere contain the text, and which ones. You confirm, edit or
  discard it; a suggestion does nothing until it is confirmed, and a
  discarded one is not suggested again. Open it with Cmd+Shift+R
  (Ctrl+Shift+R elsewhere) or from the command list.
- Text files (`.txt`, `.md`) and Word and OpenDocument texts (`.docx`,
  `.odt`) are now read too, so their text is stored with the document and can
  be searched. No office installation is needed. A broken file shows an error
  on the status page; very large files are cut off, so a crafted file cannot
  fill the memory.

- A small AI model (Qwen3 0.6B, Apache-2.0) that runs inside the app and fills
  in the fields the rules left open, without sending a document anywhere. The
  first time filing is ready with the AI turned on, the app asks before it
  downloads the 640 MB file from huggingface.co. It checks the file before use
  and continues an interrupted download where it stopped. `[modules.filing.ai]`
  picks this model or an HTTP endpoint on this machine. The app now needs macOS
  10.15 or later on Intel Macs.
- The groundwork for letting an AI fill in the fields the rules left open
  (sender, kind, date, number, amount). There is an interface for such a
  provider that can only return field values, a step in the processing that
  uses it, and a provider for servers that speak the common chat API, which
  asks for your consent before a document leaves this machine. No provider
  is switched on in the app yet, so nothing changes for you today, and
  nothing is sent anywhere.
- On macOS, scanned pages are now read with the text recognition built into
  the system (Apple Vision), which needs no download. If it fails, the ocrs
  models are used when they have been downloaded.
- Scanned pages are now read with text recognition (the ocrs project). The
  two model files it needs are not part of ncrs: the command "Download text
  recognition models…" shows the server, the files with their size and
  checksum and the folder first, and downloads them only when you press
  Download. Each file is checked before it is used. Without the models a scan
  has no text and goes to the review list, as before.
- The release archives and the macOS app now contain the PDF library
  (pdfium), and the install scripts put it next to the program, so ncrs reads
  PDFs without any extra install.
- PDFs are now read: the text of a PDF is stored with the document and can
  be searched. A PDF without text, such as a scan, is prepared as page
  images so a later step can recognise its text. This needs the PDF library
  (pdfium), which sits next to the program; without it each PDF shows
  an error on the status page and is tried again on the next start.
- Rules now decide where a new document would be filed. A rule can match on
  words in the text (any case) or on a pattern, and can read the date, the
  number and the amount out of the text; the filing pattern turns them into a
  folder and file name, for example `{year}/{date}_{sender}_{number}.pdf`.
  Whatever a document says is cleaned first, so its text can never send a file
  outside the archive. A document that no confirmed rule fits, or that lacks a
  value the pattern needs, goes to the review list with the reason. Nothing is
  moved yet.
- The Lab is now one grouped command list (File, Edit, View, Go, Search,
  Filing, Help). Each command shows its plain name first and the playful name
  dimmed after it; type the start of any word of either to find it, so "se"
  finds "Search in files" but no longer "Sort by size". With an empty field,
  the commands you ran last in this session come first.
- On macOS the menu bar now holds all commands, in the same groups as the
  Lab, each with its plain name first and the playful name after it in
  brackets, and with its key as the keymap defines it (`[keys]` included).
  Choosing an entry does what the Lab entry does. Entries that cannot run in
  the search view are greyed out, and everything except Quit is off while a
  dialog is open so that its keys reach the dialog. Switching the language renames the
  entries. Keys without `Cmd` or `Ctrl` that are not function keys, like
  `Enter` or `Space`, are not shown in the menu so they keep working in text
  fields. On Linux, and for now on Windows, the Lab (`F1`) is the menu.
- The paths of the two panels now sit in the title bar as tabs, one per
  panel, with the active one lit; click a tab to switch to that panel. Paths
  start with `~` under your home folder and are cut in the middle when they do
  not fit, so the last folder stays visible. The right end of the bar shows a
  Hermes chip (state and files per minute; click it to open the Inspection;
  hidden while Hermes is off) and the Lab button with its key. Drag an empty
  part of the bar to move the window, double-click it to zoom or maximize. On
  macOS the bar sits behind the traffic lights; on Windows and Linux the
  window has no system frame any more: the bar carries minimize, maximize and
  close, and the window edges resize.
- Hermes column: a column in the file panels that shows what Hermes made of
  each file (waiting, being read, filed, waiting for you, failed, left out)
  and, for a folder, how many documents lie in it and below it. The line at
  the bottom says why and where the closest rule would file it. Quick fix
  (Cmd+. / Ctrl+.) opens the document that waits for you and lets you fill in
  what is missing. The context menu has a Hermes entry with a submenu, and
  every action is also in the command list: hand a file to the inbox, process
  it again (Cmd+Shift+H / Ctrl+Shift+H), make a folder an inbox, filing
  location or archive, show it in the Inspection, and leave a file or folder
  out of Hermes and take it back. A file that is left out is never read, filed
  or moved; processing again never moves a filed document.

### Changed

- Document kinds such as invoice or receipt are shown in the language of the
  interface (Rechnung, Beleg ...) in the review list, the quick fix and saved
  queries; kinds you wrote yourself stay as they are, and nothing stored
  changes.
- Hermes says what it is doing. The status bar shows whether it runs and how
  many documents are being read, waiting, waiting for you or failed, in red
  when something is wrong; a click opens the Inspection, which starts with one
  plain sentence about where things stand. When the PDF library is missing,
  one notice says so and the PDFs wait instead of each landing in the review
  list; the review list names the real reason, such as no text read. Long
  names in lists are cut with "…", the lists scroll with the mouse wheel and
  trackpad, and Left and Right move between a card's sidebar and its page.
- In the saved queries page, Left in an empty field moves to the sidebar.
- A file whose error was followed by a successful run no longer stays marked
  as failed in the Hermes column.
- Every dialog now uses the same frame in one of three sizes: a title with a
  "×" to close, the content, a line with the keys that work, and the buttons
  at the bottom right with Cancel first. Esc closes every dialog that can be
  left safely, also the Inspection and a filing card that offers "Not now";
  the recovery key card and running jobs keep their own way out. Long names
  are shortened in the middle so the start and the extension stay visible.

## [0.4.0] — 2026-10-24

### Added

- "Fetch color theme…" in the command palette downloads a VS Code color theme
  from Open VSX by extension ID or link, installs it next to the config file
  with its license, switches it on at once and shows the license. The only
  network access of ncrs, and only after you ask for it.
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
- Release archives and the macOS app include `THIRD_PARTY_LICENSES.html`: the
  licenses and copyright notices of every dependency built into ncrs, and of
  the embedded Fira Sans font.

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
