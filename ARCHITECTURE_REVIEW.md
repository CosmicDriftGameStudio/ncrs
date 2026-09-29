# NC-rs – Architektur-Review

Stand: Commit-Stand vom Review-Tag, 4.315 LOC Rust, 85 Tests grün, Clippy sauber
(`-D warnings`). Ziel des Reviews: die Weichen für einen vollständigen
„Norton Commander 2026“-Clone mit Enterprise-Anspruch (Sicherheit, Performance,
Stabilität) **jetzt** stellen, statt später zu refactoren.

Revision 2: nach Gegenprüfung durch den Maintainer. Was sich geändert hat,
steht in Abschnitt 6; angepasst wurden Reihenfolge (4), T5, W6 und der
VFS-Schnitt (1).

Schweregrade: 🔴 kritisch · 🟠 hoch · 🟡 mittel · ⚪ niedrig

---

## 0. Gesamteindruck

Das Fundament ist für den Reifegrad ungewöhnlich gut:

- Elm-Architektur (TEA) sauber durchgezogen: `App::update` ist der einzige
  Mutationsort, Views sind pure Funktionen.
- `fs` kennt die UI nicht; Fehler sind strukturiert, nicht als Text.
- i18n ist per `build.rs` erzwungen (fehlender Kontext = Build-Fehler).
- Tasten sind Registry-basiert; Header-Hints und Bindings können nicht
  auseinanderlaufen.
- Stale-Result-Schutz per `request_id`.
- Kommentare erklären *warum*, nicht *was*.

**Die zentrale Feststellung:** Das Projekt ist bisher ein *Viewer*. Sobald es
schreibt (F5/F6/F8), tragen mehrere aktuelle Entscheidungen nicht mehr
(Index-basierte Tags, Symlink-Modell, `String`-Dateinamen, Fire-and-forget-
Tasks). Und sobald es mehr als das lokale Dateisystem zeigt (Archive,
Netzlaufwerke), trägt `PathBuf` als Adresse nicht mehr. Genau dort liegen die
Weichen.

---

## 1. Passt die Architektur für den vollen Clone?

Kurz: **Richtung ja, Schnitt noch nicht.**

Was ein NC-2026-Clone braucht, das heute nicht vorgesehen ist:

| Anforderung | Heute | Konsequenz |
|---|---|---|
| Archive als Verzeichnisse (zip/tar/7z), SFTP/SMB, Papierkorb, Suchergebnisse als Panel-Inhalt | `fs` ruft direkt `std::fs` mit `PathBuf` | Jede dieser Quellen bräuchte ein Sonder-If durch die ganze App. → **VFS-Port** (`trait Vfs`) und **`Location`** statt `PathBuf` |
| Konfigurierbare Keymap, Menüleiste, Command-Palette, Kontextmenü, Plugins | Taste → `Message` direkt | Alles, was eine Aktion auslöst, müsste `Message` kennen. → **Command-Schicht** zwischen Eingabe und Message |
| Tabs, Tree-Panel, Quick-View, Info-Panel, >2 Panels | `left_panel`/`right_panel` als zwei Felder, `PanelSide` als Enum | Jede Panel-Erweiterung ist eine Änderung an `App`. → Panels als Collection mit `PanelId`, Panel-Inhalt als Enum |
| Lange Operationen mit Fortschritt, Abbruch, Konflikt-Rückfragen, Queue | `Task::perform` fire-and-forget | Zustand landet verstreut in `App`. → **Job-Subsystem** (Actor) |
| Stabilität gegenüber iced-API-Churn (0.13→0.14 war schon eine Migration) | `iced::Size` in `messages.rs`, `iced::keyboard::Key` in `keymap.rs` | iced leckt in die Logik. → iced nur in der UI-Crate |
| Testbarkeit der Kernlogik ohne GPU/Fenster/iced-Compile | Ein Crate | Jeder Logiktest kompiliert iced mit. → **Workspace-Split** |

TEA selbst skaliert – Zed, Helix (ähnliches Modell) und große Elm-Apps zeigen
das. Das Risiko ist nicht das Muster, sondern ein 3.000-Zeilen-`update` und
ein `Message`-Enum mit 80 flachen Varianten. Dagegen hilft **verschachteltes
TEA**: jede Domäne (`Panel`, `Prompt`, `Jobs`, `Config`) hat eigene `State`,
`Msg`, `update`, `view`; `App` komponiert nur.

### Empfohlene Muster (und welche nicht)

**Ja:**

- **Ports & Adapters (hexagonal):** `Vfs` ist der Port; `LocalFs`, `ArchiveFs`,
  `SftpFs`, `TrashFs` sind Adapter. Die UI ist ebenfalls ein Adapter. Der Kern
  (`ncrs-core`) hat keine iced- und keine Netzwerk-Abhängigkeit.
  **Aber: der Trait wird erst eingeführt, wenn der zweite Adapter gebaut wird**
  (Trash für F8 ist der natürliche Kandidat). Ein Trait mit einer
  Implementierung ist mit hoher Wahrscheinlichkeit die falsche Abstraktion.
  Bis dahin: die *Datentypen* (`Entry`, `EntryName`, `FsError`) jetzt, und
  Operationen als freie Funktionen in der Signatur, die der Trait später
  braucht (`progress`, `cancel` als Parameter). `LocalFs::remove_one` ist
  dann ein Einzeiler-Delegat; nichts wird zweimal geschrieben.
- **Trait nur mit Primitiven, Algorithmen darüber:** rekursives `copy` und
  `remove` gehören *nicht* in den Trait. Der Trait hat `list`, `stat`,
  `open_read`, `open_write`, `mkdir`, `remove_one`, `rename`. Rekursives
  Kopieren und Löschen sind generische Algorithmen im Job-Layer über
  `&dyn Vfs`. Damit ist Zip→SFTP-Kopieren geschenkt, keine Implementierung
  hat Stubs, und `LocalFs` kann für Hot-Paths (`std::fs::copy`, Reflink)
  über `Caps` optimieren.
- **Command-Pattern für Aktionen:** `enum Command { OpenSelected, Copy, MkDir, … }`
  ist die *benutzerseitige* Schnittstelle. Keymap (Config), Menü, Palette,
  Kontextmenü, Plugins und Tests sprechen alle `Command`. `Message` bleibt das
  *interne* Ereignis (inkl. Task-Ergebnisse) und ist nicht konfigurierbar.
  Eine Registry `CommandInfo { id, label: Msg, default_keys, when: Context }`
  ersetzt `keymap::Action` und liefert automatisch Header-Hints, Menü und
  Palette.
- **Verschachteltes TEA / Komponenten-Update:** `PanelState::update(PanelMsg,
  &Ctx) -> Task<PanelMsg>`; `App::update` mappt nur.
- **Actor für Jobs:** `JobManager` läuft auf tokio, nimmt `JobRequest` über
  einen mpsc-Kanal, sendet `JobEvent { id, Progress | NeedsDecision | Done |
  Failed }` über eine `Subscription`. Abbruch via `CancellationToken`.
  Konfliktentscheidungen (überschreiben/überspringen/alle) sind Messages,
  keine Callbacks.
- **Newtypes für Gültigkeit:** `EntryName` (kein Separator, kein `..`, kein
  NUL, keine Windows-reservierten Namen), `Location` (Schema + Pfad),
  `JobId`, `PanelId`, `RequestId`. Ungültige Werte sind nicht konstruierbar,
  nicht nur „werden geprüft“.
- **Modal-Stack statt `Option<Prompt>`:** `Vec<Modal>`; Konflikt-Dialog über
  Fortschritts-Dialog über Panel ist in NC normal.

**Nein (bewusst):**

- **Event Sourcing / Undo-Log über alles:** Overhead ohne Nutzen; Undo für
  Dateioperationen läuft über den Papierkorb, nicht über Replays.
- **Trait-Object-Plugin-System (wasm/extism) jetzt:** Roadmap-Entscheidung
  (Config-Schema = Plugin-Schnittstelle) ist richtig. Die `Command`-Registry
  ist der spätere Andockpunkt, sollte es je nötig werden.
- **ECS oder reaktive Signals:** passt nicht zu iced, löst kein vorhandenes
  Problem.
- **Wechsel weg von iced:** Risiko ist API-Churn, nicht Eignung. Mitigation ist
  der Crate-Schnitt, nicht der Framework-Wechsel.

### Zielstruktur (Workspace)

```
ncrs/
├── Cargo.toml                 # [workspace], [workspace.lints], [workspace.dependencies]
├── crates/
│   ├── ncrs-core/             # KEIN iced. Alles testbar ohne Fenster.
│   │   ├── vfs/               # trait Vfs, Location, Entry, FsError, EntryName
│   │   │   ├── local.rs       # std::fs-Adapter
│   │   │   └── (archive.rs, sftp.rs, trash.rs später)
│   │   ├── job/               # JobManager, JobRequest/Event, Cancel, Conflict
│   │   ├── panel/             # PanelState, PanelMsg, update(), Selection (identitätsbasiert)
│   │   ├── command.rs         # enum Command + Registry (id, Msg-Label, default_keys, when)
│   │   ├── keymap.rs          # Key-Typ (eigen, nicht iced), Config → Command
│   │   └── config/            # serde+toml, deny_unknown_fields, version, Migration
│   ├── ncrs-i18n/             # build.rs + strings.json, Msg, Language (nur Text, keine UI)
│   ├── ncrs-ui/               # iced. view()-Funktionen, Theme, Layout, Dialoge, Simulator-Tests
│   │   └── input.rs           # iced::keyboard::Key → core::Key (einzige Stelle)
│   └── ncrs/                  # bin: main.rs, Wiring, Logging-Init, Panic-Hook
├── xtask/                     # release-checks, translation-dump, license-report
└── deny.toml, rust-toolchain.toml, SECURITY.md, CHANGELOG.md
```

Grenzen, die der Compiler erzwingt:

- `ncrs-core` hat `iced` nicht in `Cargo.toml` → kein Leck möglich.
- `ncrs-ui` hängt von `core`, nie umgekehrt.
- `ncrs-i18n` kennt weder `core` noch `ui`; beide konsumieren `Msg`.

Der Split ist ~1 Tag mechanischer Arbeit, solange das Projekt 4.000 Zeilen
hat. Bei 20.000 sind es zwei Wochen.

### Kern-Typen (Entwurf, zur Diskussion)

```rust
// ncrs-core/vfs/mod.rs
pub struct Location { scheme: Scheme, path: VfsPath }   // file:///…, zip://archiv.zip!/dir, sftp://host/…
pub enum Scheme { Local, Archive { container: Box<Location> }, Sftp { host: Host }, Trash }

pub struct Entry {
    pub name: EntryName,            // OsString-basiert, validiert
    pub display: DisplayCache,      // name_lossy, size_text, time_text – einmal berechnet
    pub kind: EntryKind,            // File | Dir | Symlink { target_kind: Option<Box<EntryKind>>, broken: bool }
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub attrs: Attrs,               // readonly, hidden, mode (unix), … für die Anzeige
}

pub struct FsError { pub op: FsOp, pub location: Location, pub kind: io::ErrorKind, pub message: Arc<str> }
// Clone + Eq einmal; UI mappt (op, kind) → Msg.

// Erst beim zweiten Adapter. Nur Primitive – keine rekursiven Operationen.
pub trait Vfs: Send + Sync {
    async fn list(&self, at: &Location, cancel: CancellationToken) -> Result<Listing, FsError>;
    async fn stat(&self, at: &Location) -> Result<Entry, FsError>;
    async fn open_read(&self, at: &Location) -> Result<Box<dyn AsyncRead + Send + Unpin>, FsError>;
    async fn open_write(&self, at: &Location, opts: WriteOpts) -> Result<Box<dyn AsyncWrite + Send + Unpin>, FsError>;
    async fn mkdir(&self, at: &Location, name: &EntryName) -> Result<(), FsError>;
    async fn remove_one(&self, at: &Location) -> Result<(), FsError>;   // eine Datei oder ein leeres Verzeichnis
    async fn rename(&self, from: &Location, to: &Location) -> Result<(), FsError>;
    fn capabilities(&self) -> Caps;   // Trash? atomares Rename? Symlinks? Reflink? schreibbar?
}

// ncrs-core/job/algo.rs – generisch, einmal geschrieben, für jeden Adapter:
pub async fn copy_tree(src: &dyn Vfs, from: &Location, dst: &dyn Vfs, to: &Location,
                       opts: CopyOpts, progress: &ProgressSink, cancel: &CancellationToken,
                       conflicts: &mut dyn ConflictPolicy) -> Result<(), FsError>;
pub async fn remove_tree(fs: &dyn Vfs, at: &Location, progress: &ProgressSink,
                         cancel: &CancellationToken) -> Result<(), FsError>;

// ncrs-core/command.rs
pub enum Command { MoveCursor(i32), Open, GoUp, SwitchPanel, Tag, TagAll, ClearTags, MkDir, Copy, Move, Delete, View, Edit, Quit, … }
pub struct CommandInfo { pub id: Command, pub label: Msg, pub default_keys: &'static [KeyChord], pub when: When }
pub enum When { Always, PanelFocused, ModalOpen, … }
```

---

## 2. Weichen – jetzt entscheiden

| # | Weiche | Ist-Zustand | Warum jetzt |
|---|---|---|---|
| W0 | **Workspace-Split, iced nur in `ui`** | Ein Crate; `iced::Size` in `messages.rs`, `iced::keyboard::Key` in `keymap.rs` | Ohne Compiler-Grenze wächst das Leck. Split ist heute billig. |
| W1 | **Tags an Identität, nicht an Index** | `SelectionSet = BTreeSet<usize>`; `retain_valid` prüft nur `i < len` | Nach Reload zeigt Index 3 auf eine *andere* Datei. Bei F8 wird die falsche gelöscht. → Tag = `EntryName`, Intersect nach Reload. |
| W2 | **Ein Fehlertyp fürs Dateisystem** | `ReadError`, `CreateDirError` (handgeschriebenes `Clone`/`PartialEq` um `io::Error`; Variante `AlreadyExists` trägt auch `NotFound`) | Jede weitere Operation wiederholt 80 Zeilen Boilerplate. → `FsError { op, location, kind, message }`. |
| W3 | **Operationen als Jobs, nicht als Tasks** | `Task::perform(create_dir)` fire-and-forget; `request_id` verwirft nur Ergebnisse | Copy/Move/Delete brauchen Fortschritt, Abbruch, Konflikt-Rückfragen, Queue, Nicht-Doppelstart. |
| W4 | **Pfad-Sicherheit im `fs`-Layer, nicht im Dialog** | `Prompt::validate` (UI-nah) prüft `/`, `\`, `.`, `..`; danach `parent.join(name)` in `app.rs` | Jede künftige Eingabestelle (Rename, Copy-Ziel, Quick-Search, Config) muss daran denken. → `EntryName`-Newtype; `join` nur für validierte Namen; zusätzlich NUL, Windows-reservierte Namen (`CON`, `NUL`, trailing `.`/Space). |
| W5 | **Symlink-Modell** | `from_dir_entry` folgt Symlinks; `is_dir = true` für Link auf Verzeichnis | Fürs Navigieren richtig, für rekursives Delete/Copy gefährlich (steigt ins Ziel ab). → `EntryKind::Symlink { target_kind }`; Operationen auf `symlink_metadata`. |
| W6 | **Nicht-UTF-8-Dateinamen** | `name: String` via `to_string_lossy`; `apply_listing` selektiert per Name-Vergleich | Auf macOS/APFS nicht reproduzierbar (Kernel lehnt ungültiges UTF-8 ab – vom Maintainer geprüft). Auf Linux/ext4, Windows (ungepaarte UTF-16-Surrogate) und Samba-Shares mit Alt-Kodierung real. **Entscheidend: W1 stellt Tags auf Namen-Identität um – ein lossy `String` als Identität ist genau die Kollision.** → Wird zusammen mit W1 erledigt (`OsString` + einmalig berechneter Anzeige-String), kostet dort nichts extra. |
| W7 | **`Location` statt `PathBuf`** | `PathBuf` überall | Archive/Netz/Trash als Panel-Inhalt sind sonst Sonderfälle in jeder Funktion. Kommt mit dem Trait beim zweiten Adapter, nicht vorher. |
| W8 | **Command-Schicht** | Taste → `Message` | Konfigurierbare Keymap (T11), Menü, Palette brauchen eine stabile, serialisierbare Aktions-ID. |
| W9 | **Message strukturieren, Listing als `Arc`** | 20 flache Varianten; `DirectoryLoaded` trägt `Vec<FileEntry>`, `Message: Clone` | Wird 60+. `Message::Panel(id, PanelMsg)`, `::Modal(ModalMsg)`, `::Job(JobEvent)`. `Arc<Listing>`, sonst wird bei 100k Einträgen das Listing kopiert. |
| W10 | **Prompt-Zustand vereinheitlichen → Modal-Stack** | `prompt`, `prompt_side`, `prompt_request_id` als drei Felder; `unwrap_or(active_panel)` kaschiert invaliden Zustand | `Vec<Modal>` mit `side` und `request_id` *im* Modal. |
| W11 | **Logging/Diagnose** | `eprintln!` nur im Debug-Build, kein Panic-Hook | Ein Absturz beim Kunden muss eine Spur hinterlassen. `tracing` + Datei-Appender in `$XDG_STATE_HOME/ncrs/`, `panic::set_hook`. Muss *vor* den Jobs existieren. |
| W12 | **Config-Format** (Roadmap T10) | offen | TOML + serde, `deny_unknown_fields`, `version`-Feld, Migrationspfad. Nebeneffekt: `serde_json` als Build-Dependency ersetzt den handgeschriebenen Parser in `build.rs`. |
| W13 | **Panels als Collection** | `left_panel`/`right_panel`, `PanelSide` | Tabs, Tree, Quick-View. `Vec<Panel>` + `PanelId`; `PanelSide` bleibt als Layout-Begriff (welche Spalte), nicht als Identität. |

---

## 3. Mängelliste

### 3.1 Sicherheit

| # | Sev | Befund | Fundstelle |
|---|---|---|---|
| S1 | 🔴 | **Kein Supply-Chain-Gate.** Kein `cargo audit`/`cargo deny` in CI, kein Dependabot/Renovate, GitHub-Actions nicht per SHA gepinnt (`actions/checkout@v4`, `softprops/action-gh-release@v2`). Lizenzaudit existiert nur als Prosa in ROADMAP. | `.github/workflows/*` |
| S2 | 🔴 | **Installer-Prüfsumme optional und aus derselben Quelle.** Fehlende `SHA256SUMS` → „skipping“, Installation läuft weiter. Ein kompromittiertes Release lässt die Datei einfach weg. Prüfsumme neben dem Binary schützt nur gegen Übertragungsfehler. `releases/latest` ohne Versions-Pin. | `install.sh:88-118`, `install.ps1` |
| S3 | 🟠 | **Traversal-Schutz nur in der UI-Schicht** (W4). Kein Test ruft `fs::create_dir` selbst mit `..` auf – der Layer vertraut dem Aufrufer. | `dialog.rs:107`, `app.rs:186` |
| S4 | 🟠 | **Handgeschriebener JSON-Parser in `build.rs`** ist zeilenbasiert: `}` in einem String, mehrzeiliger Wert oder andere Einrückung bricht das Parsen *still* (Eintrag fehlt). Build-Skripte sind Angriffsfläche. | `build.rs:26-100` |
| S5 | 🟠 | `tokio = { features = ["full"] }` zieht `net`, `process`, `signal` ein. Ungenutzte Angriffsfläche und Compile-Zeit. Nötig: `rt-multi-thread`, `sync`, ggf. `fs`, `time`. | `Cargo.toml:10` |
| S6 | 🟡 | Keine Lint-Policy im Manifest: kein `unsafe_code = "forbid"`, kein `clippy::unwrap_used`/`expect_used`/`panic`/`indexing_slicing` für Non-Test-Code. Heute sauber – soll erzwungen bleiben. | `Cargo.toml` `[lints]` |
| S7 | 🟡 | OS-Fehlertexte roh in der Statusleiste. Bei Netzlaufwerken enthalten sie Hostnamen/Credential-Hinweise. Mit W11 entscheiden: was in UI, was ins Log. | `statusbar.rs:74` |
| S8 | 🟡 | Für Netzwerk-Adapter (Phase 3) fehlt eine Credential-Strategie: OS-Keychain (`keyring`-Crate), nie in TOML. Jetzt festlegen, damit die Config-Struktur (W12) keinen `password`-Slot bekommt. | – |
| S9 | ⚪ | Keine Binary-Signatur (Sigstore/cosign oder minisign). macOS: ohne Notarization Gatekeeper-Block. Windows: SmartScreen. | `release.yml` |
| S10 | ⚪ | Kein `SECURITY.md` (Meldeweg), kein SBOM (`cargo cyclonedx`) im Release-Artefakt. | – |

### 3.2 Stabilität

| # | Sev | Befund | Fundstelle |
|---|---|---|---|
| T1 | 🔴 | **Race in F7:** `create_dir` und `reload` starten gleichzeitig via `Task::batch`. Ist der Reload schneller, fehlt der neue Ordner im Listing und `select` greift ins Leere. Reload gehört in den `PromptFinished`-Zweig. | `app.rs:186-197` |
| T2 | 🔴 | **Blocking-Reads nicht abbrechbar.** `request_id` verwirft nur das Ergebnis; der Thread auf einem hängenden NFS/SMB-Mount bleibt belegt. Key-Repeat auf Enter/Backspace startet Dutzende Reads parallel. Kein Timeout, kein Limit. → `CancellationToken` pro Panel; `begin_load` bricht den Vorgänger ab; große Verzeichnisse in Chunks streamen. | `fs/reader.rs:36`, `app.rs:497` |
| T3 | 🟠 | **Tags per Index** (W1). Heute harmlos, mit F8 Datenverlust. | `selection.rs:113` |
| T4 | 🟠 | **Kein Panic-Hook, kein Log** (W11). `iced::Result` aus `main` deckt nur den Start ab. | `main.rs` |
| T5 | 🔴 | **Snapshot-Referenztests sind immer grün.** `matches_image`/`matches_hash` liefern `Result<bool>`, `Ok(false)` bei Abweichung. Die Referenztests rufen nur `.expect(...)` und werten den `bool` nie aus. **Nachgewiesen:** `two_panels-tiny-skia.png` durch eine 1200×760-Magenta-Fläche ersetzt → `test result: ok. 1 passed`. Betrifft `the_two_panel_view_matches_its_snapshot` und die Referenz-Hälfte von `the_prompt_is_drawn_over_the_panels`, `a_long_name_does_not_change_the_layout`, `the_error_line_changes_the_prompt`; die `assert!(!same)`-Hälften sind valide. Zusätzlich enthält der Snapshot `cwd` und `$HOME` als Paneltitel (`App::new()`), ist also pro Rechner anders – fällt nur nicht auf, weil nie verglichen wird. → `assert!(sim.snapshot(..).matches_image(..).expect(..))` + feste Testdaten statt `App::new()`. Danach ist der Renderer-Suffix von `iced_test` eine echte Referenz pro Backend; Font-Rasterung auf Fremd-Runnern bleibt ein Restrisiko. | `ui_tests.rs:60-170` |
| T6 | 🟡 | Tests schreiben in `temp_dir()` ohne Guard – bei `assert!`-Fehlschlag bleibt Müll liegen, nächster Lauf schlägt mit `AlreadyExists` fehl. `tempfile::TempDir`. `start_dir_is_readable` hängt vom CWD des Runners ab. | `fs/reader.rs:120`, `fs/ops.rs:107`, `app.rs:690` |
| T7 | 🟡 | Kein Filesystem-Watcher – Listing veraltet still. Mindestens: Operationen prüfen `symlink_metadata` gegen den Cache (`modified`, `len`) vor dem Zugriff. Langfristig `notify`, mit Debounce. | – |
| T8 | 🟡 | `Message::TagAll` klont `panel.entries` komplett, um einen Borrow-Konflikt zu umgehen. Bei 100k Einträgen pro Tastendruck. | `app.rs:262-264` |
| T9 | 🟡 | `home_dir()` fällt auf `current_dir()`, dann `/` zurück – auf Windows kein sinnvoller Pfad. `root_of` ist dead code. `dirs`/`directories`-Crate oder eigene Plattform-Funktion. | `fs/reader.rs:78-100` |
| T10 | 🟡 | Windows-Spezifika ungeprüft: Laufwerkswechsel (Alt+F1/F2 braucht Drive-Enumeration), UNC-Pfade, `\\?\`-Long-Paths, case-insensitive Vergleich beim Re-Select. `Location` (W7) ist der Ort dafür. | – |
| T11 | ⚪ | `Backspace` im Prompt macht `name.pop()` – entfernt ein `char`, kein Grapheme. Da `text_input` ohnehin `on_input` bekommt, ist das eigene Zeichen-Routing redundant; nur Enter/Escape brauchen die Subscription. | `app.rs:70` |
| T12 | ⚪ | `PromptKeyState` transportiert den getippten Text pro Keypress durch die Subscription (`Arc<str>` neu allokiert bei jedem `subscription()`-Aufruf). Verschwindet mit T11. | `app.rs:44` |

### 3.3 Performance

| # | Sev | Befund | Fundstelle |
|---|---|---|---|
| P1 | 🟠 | **Ein `metadata()`-Syscall pro Eintrag** plus Symlink-Follow. Lokal ok, auf Netzlaufwerken ein Roundtrip pro Datei. → `DirEntry::metadata()` zuerst (Linux/Windows oft ohne Syscall), `symlink_metadata` nur für Links; Listing in Chunks streamen; Anzeige nach dem ersten Chunk. | `fs/entry.rs:24` |
| P2 | 🟠 | **Allokationen pro Frame:** jede sichtbare Zeile formatiert `size`, `time` (chrono), klont `name` – 28 Zeilen × 2 Panels × 4 Strings pro Redraw. Gemessen 0,1 ms, skaliert aber mit HiDPI (60+ Zeilen) und ist unnötig. → `DisplayCache` einmal im Blocking-Thread beim Laden. | `ui/panel.rs:210-240`, `ui/format.rs` |
| P3 | 🟡 | `listing_order` ruft `to_lowercase()` **pro Vergleich** auf: O(n log n) Heap-Allokationen. → `sort_by_cached_key`; ohnehin kommt Natural-Sort (`10.txt` nach `9.txt`) und ggf. Locale-Collation. | `fs/entry.rs:70` |
| P4 | 🟡 | View ist bereits fenstergebunden (`skip/take`) – gut. Nicht auf `scrollable` mit allen Zeilen umstellen; iced hat kein Widget-Diffing. | `ui/panel.rs:154` |
| P5 | ⚪ | `fira-sans` aktiv, aber `Font::MONOSPACE` genutzt → Font eingebettet, nie verwendet (Binärgröße). | `Cargo.toml:8` |
| P6 | ⚪ | Release-Profil ohne `panic = "abort"` und `strip = true`. Mit Panic-Hook (W11) ist `abort` sicher. | `Cargo.toml:14` |
| P7 | ⚪ | Keine Benchmarks (`criterion`) für `read_sync` und Sortierung – Regressionen bei 100k-Verzeichnissen fallen erst beim Nutzer auf. Der `render_timing`-Test ist ein guter Anfang, aber ein Unit-Test mit Wanduhr flackert auf CI. | `app.rs:render_timing` |

### 3.4 Prozess / Betrieb

| # | Sev | Befund |
|---|---|---|
| O1 | 🟠 | Kein `rust-toolchain.toml`, keine `rust-version` (MSRV) im Manifest – `rustup default stable` in CI = beliebige Version, Builds nicht reproduzierbar. |
| O2 | 🟠 | Release-Workflow: `macos-13` (für aarch64) ist von GitHub abgekündigt; Zuordnung vertauscht (aarch64 gehört auf `macos-latest` = ARM, x86_64 cross oder `macos-13`). |
| O3 | 🟡 | Keine Coverage (`cargo llvm-cov`), kein `cargo doc --no-deps` mit `-D warnings`, kein `cargo test --release` (LTO-Pfad wird nie getestet). |
| O4 | 🟡 | Kein `CHANGELOG.md`, `SECURITY.md`, `CONTRIBUTING.md`. Kein Conventional-Commits/Release-Automation (`cargo-release` oder `release-plz`). |
| O5 | 🟡 | ROADMAP.md vermischt Bewertung, Tasks und Entscheidungslog. → ADRs (`docs/adr/NNNN-*.md`) für Entscheidungen, ROADMAP nur für Tasks. Das Review hier ist Kandidat für ADR-0001…0005. |
| O6 | ⚪ | `.ori/` und `target/` ignoriert, `Cargo.lock` committed – gut. |

---

## 4. Empfohlene Reihenfolge

Sortierkriterium (Revision 2): **erst Datenverlust-Risiken und falsche
Sicherheit im vorhandenen Code, dann Guardrails, dann Struktur.** Kleine,
einzeln reviewbare PRs.

| Schritt | Inhalt | Aufwand | Löst |
|---|---|---|---|
| 1 | **Bugs im vorhandenen Code:** F7-Race (Reload in `PromptFinished`), `tempfile` in Tests, **Snapshot-Bool assertieren + feste Testdaten statt `App::new()`** | 2 h | T1, T5, T6 |
| 2 | **Tags auf Identität**, dabei `name: OsString` + Anzeige-String | ½ Tag | W1, W6, T3, T8 |
| 3 | **Guardrails:** `[lints]`, `rust-toolchain.toml` + `rust-version`, `cargo deny` + `cargo audit` in CI, Actions per SHA, tokio-Features kürzen, macOS-Runner fixen, `SECURITY.md` | ½ Tag | S1, S5, S6, S10, O1, O2 |
| 4 | Kleinere Sauberkeiten: Prompt-Zeichenrouting über `text_input`, `PromptKeyState` abspecken, `fira-sans` raus | Stunden | T11, T12, P5 |
| 5 | **Diagnose:** `tracing` + Datei-Appender + Panic-Hook; `panic = "abort"`, `strip` im Release | ½ Tag | W11, T4, P6 |
| 6 | **Workspace-Split:** `core` / `i18n` / `ui` / `bin`; eigener `Key`-Typ; iced raus aus `messages` und `keymap` | 1 Tag | W0 |
| 7 | **Datentypen, kein Trait:** `Entry` mit `EntryKind` + `DisplayCache`, `EntryName`, `FsError`, abbrechbares `list`, `sort_by_cached_key`, `metadata`-Strategie | 1 Tag | W2, W4, W5, T2, T10, P1–P3, S3 |
| 8 | **Command-Schicht + Modal-Stack + Message-Struktur** | 1 Tag | W8–W10 |
| 9 | **Job-Subsystem** (Actor, Progress, Cancel, Conflict-Policy), dann **F5/F6/F8** als freie Funktionen in trait-förmiger Signatur (`progress`, `cancel` als Parameter). Rekursion als generischer Algorithmus, nicht als Operation. | 3 Tage | W3 |
| 10 | **`trait Vfs` + `Location`** beim zweiten Adapter (Trash für F8). `LocalFs` delegiert an die Funktionen aus Schritt 9. | 1 Tag | W7 |
| 11 | **Config** (serde/TOML), `build.rs` auf `serde_json`; Keymap/Theme/Sprache aus Config | 2 Tage | W12, S4, Roadmap T10–T13 |
| 12 | **Panels als Collection**, Tabs | 1 Tag | W13 |
| 13 | **Release härten:** Checksum-Pflicht, Signatur, Versions-Pin im Installer, SBOM | 1 Tag | S2, S9 |
| 14 | Benchmarks, Coverage, Watcher | 1 Tag | P7, O3, T7 |

Schritt 1 hat die höchste Priorität: zwei der drei Punkte sind Datenverlust
bzw. falsche Testsicherheit in bereits ausgeliefertem Code. Schritte 2 und 3
sind unabhängig und können parallel laufen. Schritte 7–10 bilden zusammen
das, was Revision 1 „VFS-Fundament“ nannte – jetzt so geschnitten, dass
F5/F6/F8 *vor* dem Trait entstehen und nichts zweimal geschrieben wird.

---

## 5. Was bleiben soll

Damit beim Umbau nichts Gutes verloren geht:

- TEA mit `update` als einzigem Mutationsort, pure Views.
- `build.rs`-erzwungene i18n mit Kontextpflicht; die Test-Suite dazu.
- Registry-Prinzip für Aktionen (wird zur `Command`-Registry, nicht ersetzt).
- Stale-Result-Schutz (wird um Cancel ergänzt, nicht ersetzt).
- Backend-Feature-Modell und der Test, der den Backend-Drift fängt.
- Fenstergebundenes Rendering der Panel-Zeilen.
- Die Kommentar-Kultur: *warum*, nicht *was*.

---

## 6. Gegenprüfung durch den Maintainer – was sich geändert hat

Der Maintainer hat acht Befunde selbst am Code verifiziert (T1, T3/W1, T8,
S3, P5, S2, O2, T2 – alle bestätigt) und vier Einwände erhoben. Bewertung:

| Einwand | Bewertung | Konsequenz |
|---|---|---|
| **Reihenfolge: T1 und W1 vor allem anderen.** Datenverlust-Risiken im vorhandenen Code schlagen Strukturfragen. | **Richtig.** Revision 1 hatte nach „Weichen“ sortiert; das war das falsche Kriterium. | Abschnitt 4 komplett neu sortiert. |
| **Split-Begründung „Compile-Zeit“** zieht nicht: 1,4 s warm, 3,2 s kalt gemessen. | **Richtig.** Das Argument war schwach und ist gestrichen. Die Compiler-Grenze gegen iced-Leck bleibt; der Split bleibt im Plan, rutscht hinter die Bugfixes. | Schritt 6. |
| **W6 ist auf macOS unmöglich** (APFS lehnt ungültiges UTF-8 ab). | **Richtig für macOS.** Aber die Releases sind Linux/Windows/macOS, und W1 (Tags per Name) macht einen lossy `String` als Identität zur Kollisionsquelle. | W6 wird Teil von W1 statt eigener Schritt; von „Landmine“ auf „bei W1 kostenlos mit erledigen“ herabgestuft. |
| **T5 ist zu pessimistisch**, `iced_test` hat den Renderer-Suffix und echte Referenzen im Repo. | **Falsch – das Gegenteil.** Der Rückgabe-`bool` von `matches_image`/`matches_hash` wird in keinem Referenztest ausgewertet. Nachgewiesen: Referenz durch Magenta-Fläche ersetzt, Test bleibt grün. | T5 von 🟠 auf 🔴, in Schritt 1 gezogen. Der Renderer-Suffix ist korrekt und wird *nach* dem Fix zu einer echten Referenz. |
| **Frage: F5/F6/F8 vor oder nach dem VFS?** Dahinter: `copy`/`remove` mit `ProgressSink` im Trait → `LocalFs` hätte halbe Stubs. | Die Stub-Sorge trifft eher Archive/Trash als `LocalFs`, aber der Einwand zeigt den falschen Trait-Schnitt in Revision 1. | Trait nur mit Primitiven; rekursive Operationen als generische Algorithmen im Job-Layer. Trait erst beim zweiten Adapter. F5/F6/F8 *vorher*, in trait-förmiger Signatur – die „zweimal schreiben“-Frage ist damit gegenstandslos. |

Offen, Entscheidung des Maintainers:

- **Papierkorb (Roadmap T8):** entscheidet, ob Schritt 10 direkt nach 9
  kommt oder erst mit den Archiven.
- **Snapshot-Referenzen auf Fremd-Runnern:** nach dem T5-Fix werden die
  Tests auf CI zum ersten Mal *wirklich* vergleichen. Wenn Font-Rasterung
  zwischen macOS/Linux/Windows abweicht, brauchen die Referenzen einen
  OS-Suffix oder die Tests laufen nur auf einem Runner.
