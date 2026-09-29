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

## 6. Drei Entscheidungen, die vor Phase 0 fallen müssen

1. **Trash oder nicht?** (F8) Ohne Papierkorb ist F8 unwiderruflich. Es gibt die Crate
   `trash` (MIT) — die nennt crates.io als Lizenz; **lokal verifiziert habe ich das
   nicht**, sie war nicht im Cargo-Cache. Also vor der Umsetzung einmal gegenprüfen.
   Keine Lizenzfrage im Prinzip, sondern eine UX-Frage. Meine Empfehlung: rein.
2. **Testharness** (3.2): reine Logiktests wie heute, oder zusätzlich Snapshot-Tests der
   Views? Bestimmt, ob „alles getestet" realistisch ist.
3. **Sprachen**: nur `en`/`de` wie von dir genannt, oder ist das Set von Anfang an offen
   (Sprachdatei pro Sprache, fehlender Schlüssel → Fallback)? Die Fallback-Mechanik
   existiert bereits. Offen ist die Frage, ob der Kontext in Rust-Sourden bleibt oder
   nach `strings.json` wandert (Abschnitt 2).

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

Legende: **[P]** Pflicht für ein benutzbares Basis-Feature, **[S]** später. Reihenfolge =
Abhängigkeit, nicht Bequemlichkeit. Jeder Task endet grün: `cargo fmt --check`,
`cargo clippy --all-targets`, `cargo test`, ein Commit.

## Block A – CI und Installation (zuerst, weil alles Weitere davon profitiert)

- [x] **T1 – GitHub Actions: fmt + clippy + test auf macOS/Linux/Windows**
  Kein Release-Artefakt, nur Gate. Eine Datei `.github/workflows/ci.yml`, Matrix über
  drei OS, `rustfmt` und `clippy -D warnings`. Ohne das ist „alles mit tests" nicht
  durchsetzbar.
  *Warum zuerst:* jeder spätere Task soll beim PR automatisch grün sein.

- [x] **T2 – Linux-Build ohne GPU-Abhängigkeit klären**
  `iced` hat per Default **beide** Renderer im Baum (verifiziert in `iced_renderer-0.13.0/src/lib.rs:28,45`:
  `iced_wgpu` *und* `iced_tiny_skia`). wgpu braucht Vulkan/Metal/DX11 zur Laufzeit, auf
  CI-Runnern oft nicht vorhanden.
  **Entscheidung:** `default-features = false` + `tiny-skia` (Software-Rendering) als
  Default, wgpu nur als optionales Feature. Ein Dateimanager rendert Text und Rechtecke –
  Software-Rendering reicht und macht Linux-CI, VMs und alte Hardware nutzbar.
  *Risiko:* Renderer-Wechsel ist ein echter Eingriff, erst testen.

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

- [ ] **T6 – F7 MkDir** – kleinster End-to-End-Durchstich: Dialog → `Task` → `Message` → Reload
- [ ] **T7 – F5 Copy / F6 Move** – nutzt `inactive_panel_mut()`, Reload beider Panels
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

## Entscheidungen, die vor den jeweiligen Tasks fallen

| Task | Frage | Meine Empfehlung |
|---|---|---|
| T2 | wgpu behalten oder Software-Rendering? | Software (tiny-skia), siehe oben |
| T8 | Papierkorb? Crate `trash`, Lizenz ungeprüft | ja, aber Lizenz vorher prüfen |
| T5 | `build.rs` oder Handpflege? | `build.rs`, sonst laufen Code und Daten auseinander |
| T10 | TOML oder JSON für die Config? | TOML, besser für Handeditierung |
