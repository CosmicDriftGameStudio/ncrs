# NC-rs: Bewertung und Planungsgrundlage

Datum: 2026-09-29 · Stand: ein Commit, 1217 Zeilen, 9 Tests · noch kein Release, keine Nutzer

## 1. Worum es wirklich geht

Deine Anforderung „standard Datei manager funktionen" ist eine **Spezifikation**, keine
Feature-Liste. Es gibt kein Norm-Dokument, an dem du dich abarbeiten kannst. Die
präziseste öffentliche Referenz ist die Tastenbelegung von **Norton Commander 5.0**
(WordPerfect DOS, 1993): F1=Help, F2=UserMenu, F3=View, F4=Edit, F5=Copy, F6=Rename/Move,
F7=MkDir, F8=Delete, F9=Menu, F10=Quit, Alt+F1..F10 = Panels/Ziele, Tab = Switch.

An der arbeiten wir uns ab. Zwei Konsequenzen daraus:

- **Vergangenheit wohnt in der Konfiguration.** NC hat sich als „installierbares Produkt"
  verkauft; die Belegung lag in `FMPROG.DAT`. „Pluginable" und „configurable keymap" sind
  damit **ein** Feature, nicht zwei — siehe 3.1.
- **Der Umfang ist eine Größenordnung größer als meine erste Schätzung.** NC 5.0 waren
  etwa 25 Tastenbelegungen, davon ~10 Operationen. Siehe Abschnitt 4.

## 2. Was die Faktenlage trägt (verifiziert, nicht geschätzt)

### Lizenzaudit: sauber, mit einer Anmerkung

166 Crates im Build-Graph, per `cargo metadata` ausgewertet:

| Lizenz | Anzahl |
|---|---|
| MIT / Apache-2.0 (dual) | 131 |
| andere permissive (BSD-2/3, Zlib, ISC, CC0, 0BSD, Unlicense) | 34 |
| **GPL-Option** | **1** |

**`self_cell 1.3.0` steht unter `Apache-2.0 OR GPL-2.0-only`.** Das ist die einzige
Anomalie, und sie ist unkritisch: die Crate selbst ist Apache-2.0, und eine Disribution
unter MIT wählt die Apache-Option — GPL ist nur die reziproke Angebotsform. Es ist eine
transitive Abhängigkeit über `cosmic-text` (Text-Shaping), nicht etwas, das du importierst.
Kein Handlungsbedarf, aber im `NOTICE` erwähnen, weil es die einzige Stelle ist, an der
ein Compliance-Check hängen bleibt.

**Ergebnis: MIT-Distribution ist mit dem heutigen Tree möglich**, ohne Crate zu
forken. Das ist die gute Nachricht der ganzen Analyse.

Hinweis zu `yazi 0.1.6` im Tree: transitiv über `cosmic-text`/`swash` (Text-Shaping),
MIT OR Apache-2.0, kein Handlungsbedarf.

### i18n heißt: übersetzte Oberfläche — teilweise gebaut, Stand heute

Du hast „multi lang" genannt und `F5 = Kopieren / Copy` als Beispiel. Ich hatte zuerst
Rechtschreibprüfung gelesen — in einem Dateimanager gibt es nichts zu buchstabieren,
das war eine erfundene Anforderung von mir. Die echte Aufgabe hat zwei Teile, und der
zweite ist der wichtigere:

1. **Übersetzte Texte** (`en`/`de`) — gebaut, siehe `src/i18n/`.
2. **Kontext pro Text**, damit Übersetzung mechanisch sinnvoll geht. Das ist keine
   Feinheit, sondern die Voraussetzung: kein Werkzeug übersetzt „Open" richtig, wenn es
   nicht weiß, ob es Button-Label, Menüeintrag oder Fehlerteil ist.

**Was gebaut ist** (`src/i18n/`, 264 Zeilen, 4 Tests):

- `Msg`-Enum als Adressierung — der Compiler findet alle Aufrufstellen, wenn ein String
  umbenannt wird. Ein `t("open")`-Aufruf per String umgeht das.
- `Msg::note() -> &'static str` je String: wo erscheint er, was bedeutet er, was ist bei
  der Übersetzung zu beachten. Das ist das Deliverable für Übersetzer. Der Test
  `every_message_has_context_for_translators` schlägt fehl, wenn ein String ohne
  brauchbaren Kontext angelegt wird.
- `Language` mit `other()` (F9 temporär) und Fallback auf Englisch.
- **Strukturierte Fehler statt Strings in `fs`**: `ReadError` statt `Option<String>`.
  Sonst hätte die dateilose Schicht englische Fehlertexte bauen müssen. Der Pfad kommt
  vom Panel, der Text entsteht in der UI-Schicht — deshalb kann Deutsch den Pfad
  voranstellen (`/etc/shadow nicht lesbar: …`) während Englisch das Verb führt.
- **Platzhalter-Konsistenz erzwungen**: `placeholders_match_across_languages` fängt ab,
  wenn eine Sprache `{reason}` und eine andere `{error}` verwendet. Dieser Test hat
  einen echten Bug in meinem eigenen Code gefunden, nicht nur einen theoretischen.
- `<DIR>`/`<UP>` bleiben per Konvention unübersetzt, mit Test.

**Was noch fehlt, bevor „fertig":**

- **Persistenz der Sprache.** F9 schaltet nur zur Laufzeit um; die Auswahl gehört in die
  Config (3.1). Das ist der eigentliche offene Punkt.
- **Der Export-Pfad für Übersetzer.** Heute liegt der Kontext in Rust-Sourcen. Damit
  jemand ohne Rust-Kenntnisse übersetzen kann, braucht es eine Datendatei im Repo plus
  Generierung. **Offene Frage: `build.rs`, das die Sprachmodule aus `strings.json`
  erzeugt, oder Handpflege?** Ich würde `build.rs` — sonst laufen Code und Daten
  auseinander, und das Format ist später schwerer zu wechseln.

**Sortierung nach Sprache** (`ä` unter `a`) war nie eine Anforderung von dir. Falls doch:
`icu_collator` statt `to_lowercase()` in `fs/entry.rs` — eine Zeile plus Tests.

## 3. Was du nicht genannt hast, aber brauchen wirst

### 3.1 Konfigurationsformat — dies ist der architektonische Schlüssel

Jedes deiner sechs Features (Keymap, Theming, Netzlaufwerke, Plugin-Kompatibilität,
Mehrsprachigkeit, Erweiterbarkeit) ist eine **Datenform**, die irgendwo gelesen, validiert
und interpretiert werden muss. Wenn du das fünfmal verschieden löst, bekommst du fünf
inkonsistente Fehlerbehandlungen und keine Testbarkeit.

**Deshalb zuerst: ein Config-Format mit Schema-Validierung**, das alle anderen
Subsysteme benutzen. Das ist die teuerste einzelne Investition und die, die alles
Weitere billiger macht. Ich würde `serde` + `toml` nehmen und beim Start validieren:
unbekannte Felder ablehnen, Version-Feld für Migration.

### 3.2 Testbarkeit ohne Fenster

„alles mit tests" und „stabile TUI" sind in Spannung. Deine `App::update` ist
Logisch fast ideal — Message rein, Task raus — aber die View-Schicht ist nicht
testbar, weil sie `Element` produziert, das man nicht inspizieren kann. Ohne
Scaffold-Entscheidung heißt „alles getestet" entweder Kompilierbarkeit oder nichts.

Das ist eine Entscheidung, die vor dem Bauen fallen muss, und sie ist deine.

## 4. Umfangsabschätzung — und warum ich sie nicht als Zusage mache

Wenn „alles mit tests" ernst gemeint ist, ist das hier kein Feature-Backlog, sondern ein
Projekt mit Hobby-Umfang. Meine grobe Schätzung, in Tagen, **ohne** Community-Input:

| Baustein | Tage | Notiz |
|---|---|---|
| Konfig-Format (serde/toml, Validierung, Versionierung) | 2–3 | 3.1, Bremsklotz |
| Keymap-Konfiguration | 1,5 | braucht 3.1 |
| Theming (TOML → theme.rs) | 2 | Colors sind heute schon sauber separiert |
| Netzwerk-Mounts (Profil-Management) | 3 | Siehe Warnung unten |
| Archiv-Support (lesend) | 4–6 | `zip`+`tar`+`zstd`; **Crate-Lizenzen ungeprüft**, s. u. |
| Archiv-Support (schreibend) | 6–10 | deutlich mehr, Umfang hängt am Format |
| i18n **Rest** (Persistenz, `strings.json`, build.rs) | 1–2 | Texte selbst sind fertig, s. Abschnitt 2 |
| Plugin-System | 5–8 | siehe 3.1 — Config *ist* die Plugin-Schnittstelle |
| Suchfunktion | 2–3 | siehe Warnung unten |
| Panels → Rechte, Datei-Info, Editor-Integration | 6–10 | Teil von „Standardfunktionen" |
| **Summe** | **ca. 36–49 Tage** | reine Implementierung, ohne Nacharbeit |

Zwei Warnungen zu meiner eigenen Schätzung:

- **Netzlaufwerke** kosten nicht 3 Tage. Mounts sind unter macOS/Linux ein
  System-Thema (macOS: `/Volumes`, Finder-Mounts; Linux: `/proc/mounts`). Das ist nicht
  „ein Feature", sondern eine Plattformabstraktion mit Tests pro Plattform. Realistisch
  3–5 Tage, wenn man es nur mountet, 8+ wenn man es richtig macht (Reconnect,
  Fehlerbehandlung bei totem Mount, Timeout bei Netz-I/O).
- **Suche** ist bei dir „super einfach und schnell". Das ist im Konflikt mit „alles
  getestet": schnell heißt, du testest nicht jede mögliche Eingabe. Ich würde einen
  `glob`-basierten Filter mit Debounce definieren, der testbar ist (Filter-Logik
  getestet, Timing nicht) und die Anforderung ehrlich erfüllt.

**36–49 Tage** ist keine Zusage, sondern die Größenordnung, damit du nicht mit einer
Zahl von mir planst, die ich nicht verteidigen kann.

**Was an dieser Tabelle geprüft ist und was nicht:** Die Zahlen sind meine Schätzung,
keine Messung. Verifiziert habe ich die Lizenzaudit (Abschnitt 2) und die
i18n-Zerlegung — beides objektiv prüfbar. Die Crate-Lizenzen für Archiv-Support und
die `trash`-Crate sind **Annahmen** (Cargo-Cache bzw. crates.io-Angabe) und vor der
Umsetzung zu prüfen. Klar ist nur die Richtung: `7z`/RAR sind für ein MIT-Projekt
praktisch ausgeschlossen, `zip`/`tar`/`zstd` nicht.

## 5. Reihenfolge — vorgeschlagen

Nicht nach Feature-Reihenfolge, sondern nach Abhängigkeit:

**Phase 0 — Fundament** (nichts sichtbar, aber alles Weitere hängt dran)
1. Config-Format mit Validierung
2. `Message`-enum wächst, `App` bekommt Fehler-State
3. Testharness-Entscheidung umsetzen (3.2)

**Phase 1 — Der File-Manager-Kern**
4. View/Edit (F3/F4) mit externem Editor
5. Copy/Move/Rename/MkDir/Delete (F5/F6/F7/F8)
6. Panels: Ziel-Panels (Alt+F1–F10), Quick-Search
7. User-Menu (F2), Help (F1)

**Phase 2 — Konfiguration wird nutzbar**
8. Keymap aus Config
9. Theme aus Config
10. Sprachen

**Phase 3 — Erweiterung**
11. Netzwerk-Mounts
12. Archive lesend
13. Archive schreibend

**Phase 4 — Härtung für den Alltag**
14. Undo für destruktive Ops
15. Performance bei 100k+ Dateien
16. README/Installer

Rationale für die Reihenfolge: Netzwerk und Archive sind die beiden Features, die am
wenigsten vom Kern abhängen und am meisten am „nicht benutzt" leiden. Sie nach hinten zu
legen ist Absicht, nicht Verseum.

## 6. Offene Entscheidungen

Die laufende Liste mit dem Stand steht am Dokumentende. Hier der Kurzstand:

1. **Trash oder nicht?** (F8) Ohne Papierkorb ist F8 unwiderruflich. Die Crate `trash`
   nennt crates.io als MIT; **lokal verifiziert habe ich das nie**, sie war nicht im
   Cargo-Cache. Keine Lizenzfrage im Prinzip, sondern eine UX-Frage. Empfehlung: rein.
2. **Testharness** (3.2): **entschieden, ohne Snapshot-Tests.** 36 Logiktests decken `fs`,
   `i18n`, `keymap`, `backend`, Layout-Arithmetik und Fehler-Rendering. Es gibt kein
   Framework, dessen Ausgabe ich gegen eine echte Referenz geprüft habe — ein
   Snapshot-Test ohne Wahrheitswert ist eine Datei, die immer grün ist. Falls sich das
   ändert, wäre `insta` der erste Kandidat.
3. **Sprach-Set**: `en`/`de` sind gebaut, Kontext liegt in `strings.json`, F9 schaltet um.
   Offen ist nur die **Persistenz** (T13), und die hängt an der Config (T10).

## 7. Nicht im Plan, mit Begründung

- **Terminal-UI statt iced**: wäre näher an NC, aber du hast ein natives Fenster gewählt
  und `iced` ist die einzige getestete Basis. Wechsel nicht.
- **Rust-Komponenten-Crests** (`filesystem`, `dunce`): lösbar, aber Netzwerk ist
  Phase 3 und du brauchst es jetzt nicht.
- **Plugin-System als separate Crate-Schnittstelle** (`wasmer`/`extism`): massiver
  Overhead. Vorschlag in 3.1 ist: Config-Schema *ist* die Plugin-Schnittstelle —
  Einsteiger ergänzen Felder, das ist mit Schema-Validierung abgesichert.

---

# Tasks – Basis-Feature

**Stand: 11 von 17 erledigt, 6 offen.** 85 Tests inkl. 12 UI-Tests mit Pixelvergleich.

| Block | Inhalt | Stand |
|---|---|---|
| A | CI, Installer, Release, `strings.json` | ✅ fertig (T1–T5, T5b) |
| B | Der Dateimanager: MkDir, Copy, Delete, View/Edit, Config, Suche | 🔶 MkDir fertig — **T7 (Copy/Move) ist der nächste Task** |
| C | Netzwerk-Mounts, Archive | ⬜ offen, hängt an B |

Legende: **[P]** Pflicht für ein benutzbares Basis-Feature, **[S]** später. Reihenfolge =
Abhängigkeit, nicht Bequemlichkeit. Jeder Task endet grün: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, ein Commit.

## CI-Erkenntnisse (2026-09-29)

Zwei Befunde, die beim ersten CI-Lauf kamen und beide nicht im Review standen:

**softbuffer bricht auf Linux ohne x11/wayland.** `iced_tiny_skia` hängt an `softbuffer`,
dessen Backend-Enum per `#[cfg]`-Armen erzeugt wird. Ohne die Features `x11`/`wayland`
greift auf Linux **kein** Arm — android, apple, windows, wasm fallen weg, x11/wayland/kms
sind nicht aktiv. Übrig bleibt ein Enum, dessen Typ-Parameter `D`, `W` und `'a` nichts
benutzen: E0392, Build abbrechen.
*Das ist kein Compiler-Regressionsfehler*, sondern eine fehlende Konfiguration — der
Toolchain-Pin aus dem Review hätte es nicht verhindert. Fix: `x11` und `wayland` in ieds
Features. Auf macOS/Windows sind die softbuffer-Backends ohnehin per `cfg` weg, dort kostet
es nichts.
*Merksatz für den Split:* `cargo tree -e features | grep softbuffer` muss **nicht leer**
sein. Ein leeres Ergebnis ist hier kein „kein Fenster", sondern ein leeres Enum.

**Snapshot-Tests sind plattformabhängig, nicht backend-abhängig.** Der Suffix in
`tests/snapshots/` deckt den Renderer ab, nicht das Betriebssystem. Font-Rasterung
unterscheidet sich, also gelten macOS-Referenzen nicht für Windows. Gelöst über
`--skip ui_tests` auf Nicht-macOS-Runnern plus einem eigenen Schritt für macOS.

## Block A – CI und Installation (zuerst, weil alles Weitere davon profitiert)

- [x] **T1 – GitHub Actions: fmt + clippy + test auf macOS/Linux/Windows**
  Kein Release-Artefakt, nur Gate. Eine Datei `.github/workflows/ci.yml`, Matrix über
  drei OS, `rustfmt` und `clippy -D warnings`. Ohne das ist „alles mit tests" nicht
  durchsetzbar.
  *Warum zuerst:* jeder spätere Task soll beim PR automatisch grün sein.

- [x] **T2 – Linux-Build ohne GPU-Abhängigkeit klären**
  `iced` hat per Default **beide** Renderer im Baum (verifiziert in
  `iced_renderer-0.13.0/src/lib.rs:28,45`: `iced_wgpu` *und* `iced_tiny_skia`). wgpu
  braucht Vulkan/Metal/DX11 zur Laufzeit, auf CI-Runnern oft nicht vorhanden.
  **Entscheidung:** `default-features = false` + `tiny-skia` (Software-Rendering) als
  Default, wgpu nur als optionales Feature. Ein Dateimanager rendert Text und Rechtecke –
  Software-Rendering reicht und macht Linux-CI, VMs und alte Hardware nutzbar.
  *Risiko:* Renderer-Wechsel ist ein echter Eingriff, erst testen.
  *Ergebnis:* 428 → 313 Pakete. App startet nachweislich.
  **Nachtrag:** Software-Rendering ist die Ursache der gemeldeten Navigations-Trägheit
  (siehe T2b).

- [x] **T2b – Backend konfigurierbar machen, und die richtige Vorgabe finden**
  Der Renderer stand nirgends im Code — `src/main.rs` enthält keine Zeile mit `wgpu`
  oder `tiny-skia`. Er wurde allein von ieds Features bestimmt, also unbemerkt
  änderbar. Zwei falsche cfg-Bedingungen in meinem ersten Test fielen zusätzlich durch.
  **Gelöst mit Features statt generischem Umbau:** `software-rendering` (Default),
  `gpu-rendering`, `gpu-with-fallback`. ied löst die Kombination selbst auf
  (`iced_renderer-0.13.0/src/lib.rs:24-59`), der Renderer bleibt untypisiert — der
  geplante Umbau durch `app.rs` und alle `ui/*`-Module war unnötig.
  **Gemessen, nicht vermutet** (drei Starts auf deinem Rechner):

  | Build | Backend | Ergebnis |
  |---|---|---|
  | `cargo run` | tiny-skia | **unbrauchbar träge** — die HiDPI-Rasterisierung |
  | `--features gpu-with-fallback` | wgpu + Fallback | **schnell** ← die Vorgabe für Releases |
  | `--features gpu-rendering` | wgpu | baute nicht: Bug im Test, s. u. |

  **Bug gefunden und behoben:** `gpu-rendering` kompilierte nicht. Ursache war meine
  `#[cfg]`-Kaskade im `const`-Block: eine nicht zutreffende Bedingung lässt einen leeren
  Block stehen, der Block ergibt `()` statt `Backend`. Fällt nur in den Konfigurationen auf,
  die der Default-Build nicht ausführt. Jetzt ein expliziter `Missing`-Zweig plus Test,
  der ihn fängt; CI prüft alle vier Konfigurationen.

  **Offen:** `default` steht weiterhin auf `software-rendering`, weil das ohne GPU
  überall startet. Für Releases gehört `gpu-with-fallback` in die Pipeline
  (T3-Update), damit der Default dem gemessenen Verhalten entspricht.

- [x] **T3 – Release-Pipeline: Binaries für Linux/macOS/Windows**
  `.github/workflows/release.yml`, getriggert per Tag. Cross-Compile per
  `cross` (Docker-Container, kein native Toolchain-Gepuzzle) oder Matrix mit je einem
  Runner pro OS. Artefakte: Linux (x86_64 + aarch64), macOS (x86_64 + aarch64),
  Windows (x86_64).
  *Voraussetzung:* T2, sonst baut Linux nicht zuverlässig.

- [x] **T4 – `curl … | sh`-Installer**
  `install.sh` (POSIX sh, kein Bash-only) lädt das Release-Artefakt der eigenen Arch,
  prüft Checksumme, entpackt nach `~/.local/bin` oder `~/.ncrs/bin`, meldet den
  `PATH`-Eintrag. Entsprechend `install.ps1` für Windows.
  **Sicherheitsregel:** Das Skript ist read-only und idempotent. Es schreibt **nur**
  nach `$HOME`, braucht kein `sudo`, und löscht nichts. Kein `curl | sudo bash`.
  *Vorher:* T3, sonst gibt es nichts zu laden.

- [x] **T5 – `strings.json` + `build.rs`: Kontext aus dem Code in eine Datei**
  Deine Anforderung („Kontext, damit man mechanisch sinnvoll übersetzen kann") in der
  Rust-üblichen Form. `strings.json` mit `key`, `en`, `de`, `context`; `build.rs`
  generiert `lang/en.rs` und `lang/de.rs` daraus. `Msg::note()` liest den Kontext dann
  aus der generierten Datei.
  *Ergebnis:* Übersetzer arbeiten ohne Rust-Kenntnisse, Kontext reist mit dem String.

## Block B – Basis-Feature (der Dateimanager selbst)

- [x] **T6 – F7 MkDir** — Dialog → `Task` → `Message` → Reload
  Die Kette einmal vollständig gebaut. Was dabei herauskam:
  - `src/fs/ops.rs`: `create_dir` via `spawn_blocking`, Fehler **strukturiert**
    (`CreateDirError` mit `io::Error` darin), damit „Name vergeben" und
    „Elternverzeichnis fehlt" unterscheidbar bleiben — zwei verschiedene Hilfehinweise.
  - `src/dialog.rs`: `Prompt`-State mit `validate()`. Leertaste-Name, `.`/`..` und
    Pfadtrenner werden abgewiesen, **bevor** das Dateisystem angefasst wird; `..` kann so
    nichts oberhalb des Panels anlegen.
  - `src/ui/dialog.rs`: Overlay über `Stack` + `opaque`-Scrim, Textfeld, zwei Buttons.
  - **Tastenverteilung war der schwierige Teil.** `iced::keyboard::on_key_press` nimmt nur
    einen `fn`-Pointer, der keinen App-Zustand sehen kann — eine Closure über `self` geht
    nicht. Lösung: jede Taste kommt als `Message::Typed` an, `App::route_key` entscheidet
    dann, ob sie zum Dialog oder zu den Panels gehört. Eine Stelle, damit kein Binding
    halb-modal werden kann.
  - 23 neue Tests (36 → 59), darunter `an_open_prompt_takes_every_key` (verhindert, dass
    Enter im Textfeld ein Verzeichnis öffnet) und ein Ende-zu-Ende-Test mit echtem
    Dateisystem.
    **Nicht getestet (Stand vor der Migration):** das Aussehen des Dialogs und die
  Bedienung per Maus. Beides ist seit T6b über `iced_test` abgedeckt.

- [x] **T6b – iced 0.13 → 0.14, mit `iced_test`**
  Migriert, um ieds offizielles Test-Werkzeug zu bekommen: `Simulator` (headless, klicken
  und tippen), `Emulator` (End-to-End), `Snapshot` (echte Pixelvergleiche), `Ice`
  (versionierbare Testskripte).
  **Was sich geändert hat:**
  - `on_key_press(fn)` gibt es nicht mehr. `keyboard::listen()` liefert den Event-Stream,
    und `Subscription::with(value)` transportiert den Zustand — damit fällt der Umweg über
    `Message::Typed` weg, den ich in T6 bauen musste, weil 0.13 keine Closures erlaubte.
  - `iced::application` nimmt `boot` statt `title` + `run_with`. Titel via `.title()`.
  - `Key::Character` trägt `&str` statt `char`.
  - `Button` hat keine `id()`-Methode, `iced_selector` findet also nur Widgets mit Id
    (z. B. `TextInput`). Buttons werden über ihre Layout-Position adressiert.
  **Ergebnis:** 8 UI-Tests, die echte Klicks und gerenderte Bilder prüfen. Der Test
  „the prompt changed no pixel" wurde mit absichtlich entferntem Overlay geprüft und
  schlägt an. 67 Tests gesamt.
  **Aus dem Weg geräumt:** ein selbstgebauter Layout-Test-Harness über tiny-skia
  (`src/ui/layout_tests.rs`). Die Crate macht ihn überflüssig, und er lief nur im
  Software-Build.

- [x] **T6c – Mehrfachauswahl (Voraussetzung für F5/F6)**
  F5 „Copy" ohne Auswahl wäre nutzlos oder gefährlich. Drei Zustände wie in NC: Cursor,
  getaggt, beides. Die Regel: **sind etwas getaggt, gilt die Operation für die Tags; sonst
  für die Cursor-Zeile.** Ohne die Vorrang-Regel wäre F5 ein Würfelwurf.
  - `src/selection.rs`: `SelectionSet` (Index-Menge, überlebt kein Listing ohne Prüfung)
    und `Selection` (Zustand einer Zeile). `..` ist nicht tagbar — ein Verzeichnis in sich
    selbst kopieren meint niemand.
  - **Stale-Tags:** nach einem Reload zeigen Indizes sonst auf andere Dateien. Sie werden
    verworfen, nicht behalten: eine vergessene Datei im Copy ist schlimmer als ein Tag, den
    man neu setzen muss.
  - Ansicht: eigene Spalte mit `*`, plus Zeilenfarbe. Ohne die Spalte wäre die Auswahl
    unsichtbar und F5 würde auf etwas Unsichtbares wirken.
  - Tasten: `Insert`, `*`, `Ctrl`+`*` — im Registry-Pattern registriert, nicht im
    View verdrahtet.
  - 14 neue Tests (71 → 85), davon 4 UI-Tests mit Pixelvergleich.
  **Ehrliche Grenze:** der Pixelvergleich beweist, dass sich *etwas* ändert, nicht dass es
  der `*` ist — die Zeilenfarbe ändert sich mit. Gemessen und im Test so benannt, statt
  eine Genauigkeit zu behaupten, die der Test nicht hat. `in_operation` hat noch keinen
  Aufrufer: die Regel wird in T7 beim Bauen der Operation gebraucht.

- [ ] **T6d – Bedienungsleisten und Ablage (Architektur, vor F5/F6/F8)**
  Fünf Anforderungen, die zusammen eine Struktur brauchen. Hier aufgenommen, weil drei
  davon den Kopier-Vorgang und die Tastatur betreffen und nicht später unterzuschieben sind.

  **1. Command-Line unten (ersetzt die Statusbar)**
  In NC ist die untere Leiste die *Kommandozeile*, nicht eine Statusanzeige: man tippt
  Befehle und Pfade. Das ersetzt `statusbar::view`, erweitert es nicht.

  *Der Konflikt, der vorher gelöst werden muss:* heute bekommt `keymap::map_key` jede
  Taste. Tippt der Nutzer unten `5`, um `F5` zu tippen, muss `"5"` Text sein, keine
  Message.
  **Die Lösung steht schon im Code:** `route_key` entscheidet heute, ob eine Taste zum
  offenen Prompt gehört. Die Leiste ist derselbe Mechanismus — Esc nimmt den Fokus,
  zweites Esc löst aus (NC-Verhalten). Keine neue Mechanik, dieselbe für ein anderes
  Ziel. Das `When`-Feld der Command-Registry bekommt damit seinen ersten echten Nutzer.

  **2. Task-Queue (Kopieren blockiert nicht)**
  `F5` auf 50.000 Dateien darf die Oberfläche nicht anhalten. Braucht einen `JobManager`
  mit `JobEvent { Progress, Done, Failed, NeedsDecision }` über eine Subscription.
  **Die Leiste zeigt den Fortschritt**, sonst ist die Queue unsichtbar — die beiden
  Punkte hängen zusammen.

  **3. Überschreiben/Ersetzen immer als „für alle"**
  NC fragt bei Namenskollision und bietet *Alle überschreiben / Alle behalten /
  Überschreiben / Behalten / Abbrechen*. Die Entscheidung ist ein `Command` im Dialog, keine
  Bedingung im Code — sonst braucht jede Operation ihre eigene Kopie des Dialogs. Gehört in
  die Command-Schicht, nicht in `fs`.

  **4. Spaltensortierung**
  `fs::entry::listing_order` ist heute fest auf Name. Sortierbare Spalten heißt
  `SortKey { Name, Size, Modified, Extension }` plus Richtung, und `listing_order` wird ein
  Funktor über `(SortKey, bool)`. NC-Sonderregeln: Verzeichnisse immer zuerst
  (unabhängig vom Schlüssel), `..` immer oben.
  *Besteht auf P3:* `to_lowercase()` pro Vergleich ist O(n log n) Allokationen —
  `sort_by_cached_key` kommt mit.

  **5. Favoriten / Historie — zwei verschiedene Dinge, hier getrennt**
  - *Favoriten* (Total Commander: Leiste zwischen Header und Liste) — feste, benannte
    Sprungziele. Kleine Liste, gehört in die Config.
  - *Historie* (NC: Verzeichnis-Rückwärts) — pro Panel; ein Zurück-Schritt geht einen
    Eintrag zurück statt eine Ebene nach oben.
  Beide brauchen einen Stack, aber unterschiedliche. **Empfehlung:** Historie zuerst, weil
  sie ohne Konfiguration auskommt. Favoriten als reine Config-Liste darüber.

- [ ] **T7 – F5 Copy / F6 Move** — nutzt `SelectionSet` und `inactive_panel_mut()`
  Braucht T6c (Auswahl) und T6d Punkt 2 (Queue, sonst blockiert ein großes Copy).
  Reload beider Panels; F5 kopiert in das inaktive Panel, F6 verschiebt dorthin.

- [ ] **T8 – F8 Delete mit Bestätigungsdialog und Papierkorb** – siehe offene Frage unten
- [ ] **T9 – F3 View / F4 Edit** – externes Programm, `std::process::Command`
- [ ] **T10 – Konfigurationsdatei** – `serde` + `toml`, Schema-Validierung, Versionierung
  *Voraussetzung für:* T11, T12, T5-Sprachpersistenz
- [ ] **T11 – Keymap aus Config** – `Msg` bleibt, Tasten kommen aus der Datei
- [ ] **T12 – Theming aus Config** – `theme.rs` liest Colors aus TOML
- [ ] **T13 – Sprachauswahl persistent** – `lang = "de"` in der Config, statt F9
- [ ] **T14 – Quick-Search** – Typen filtert die Liste, `glob`-basiert, logik-testbar

- [x] **T5b – Registry-Pattern für Tasten: eine Aktion, eine Registrierung**
  Eine Aktion trägt Taste, Message und Übersetzungshinweis. `map_key` *und* die
  Header-Liste werden daraus abgeleitet, beide können nicht auseinanderlaufen.
  `Binding::with_modifiers` ist der Erweiterungspunkt für Alt+F1…F10 (Ziel-Panels).
  6 Tests.

## Block C – Erweiterung (nicht Basis, aber im Plan)

- [ ] **T15 – Netzwerk-Mounts** (macOS/Linux), **T16 – Archiv-Support lesend**,
  **T17 – Archiv-Support schreibend** – Umfang siehe Abschnitt 4

## Was das Testen über den Aufbau gelehrt hat

Aus T2/T2b: fünf Runden mit einem Compiler-Fehler und zwei Testrunden. Die Regeln, die
sich daraus ergeben:

1. **Ein Test, der die Implementierung nachbaut, ist keine Prüfung.** Zweimal passiert —
   bei `Backend::CURRENT` und bei der ersten Fassung der `visible_rows`-Tests. Beide waren
   grün, während der Code falsch war. Ein Test liest Werte gegen eine unabhängige Quelle:
   eine Tabelle, ein Vertrag, ein gemessener Wert.
2. **Fehler an der Grenze eines Features, nicht in der Mitte.** `Backend::CURRENT` ist in
   vier Varianten kaputtgegangen, die der Default-Build nie ausführt. CI baut jetzt jede
   Feature-Kombination — auch die, von der ich glaube, sie sei ungefährlich.
3. **`cargo check` ist kein Beweis.** Ich habe damit einen „Fix" verifiziert, der das
   Problem nicht behoben hatte. `cargo run --features X` ist der Befehl, der bei einem
   Nutzer auftaucht.
4. **Die Messung vor der Vermutung.** Zwei Runden lang habe ich den Keymap-Cache als
   Ursache der Trägheit benannt; 210 ns, sichtbar irrelevant. Erst „auf einmal" und dann
   die Messung haben es auf die Rasterisierung gebracht.
5. **Acht Dateien ohne Test waren verdächtig, eine war es wirklich.** `layout.rs` hat
   eine Handrechnung über genau die Konstanten, die das Layout benutzt, und nichts hat
   die beiden verbunden. Jetzt: eine `CHROME`-Konstante plus Drift-Test.

**Was nicht testbar ist:** wie schnell sich ein Frame *anfühlt*. 3,6 Mio Pixel pro Frame
auf HiDPI sind eine Rechnung, keine Assertion. Dass `gpu-with-fallback` schneller ist,
weiß ich nur durch deine Beobachtung — die steht als Messung im README, nicht als Test.

## Entscheidungen, die vor den jeweiligen Tasks fallen

| Task | Frage | Stand |
|---|---|---|
| T2 | wgpu oder Software-Rendering? | **beides, als Features.** Gemessen: Software ist auf HiDPI unbrauchbar, `gpu-with-fallback` ist schnell. Releases bauen mit Fallback. |
| T5 | `build.rs` oder Handpflege? | **`build.rs`, gebaut.** Kontext und Text kommen aus `strings.json`. |
| T8 | Papierkorb? Crate `trash`, Lizenz ungeprüft | **offen.** Ohne die Crate ist F8 unwiderruflich; die Lizenz ist bis heute nicht lokal verifiziert. |
| T10 | TOML oder JSON für die Config? | **offen.** Empfehlung: TOML, besser für Handeditierung. |
| T13 | Sprachauswahl persistent | **teilweise.** `en`/`de` gebaut, F9 schaltet um; die Persistenz hängt an T10. |
| T14 | Suche: `glob` oder inkrementell? | **offen.** Die Filterlogik ist in beiden testbar, das Timing nicht. |
| T15 | Settings | **offen.** Wichtige Sachen soll sich das program merken |
