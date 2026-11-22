//! The screens of the Git actions (#338) in German, with a fixture repository.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

// The CI runs `ui_tests` only on macOS, where the reference images were made.
mod ui_tests {
    use std::path::PathBuf;

    use iced::widget::text_editor;
    use iced::{Point, Size};
    use iced_test::Simulator;
    use ncrs_git::{
        BranchInfo, ChangeKind, ChangedFile, CommitDetail, DiffHunk, DiffRow, FileDiff, GraphRow,
        Log, LogCommit, RefKind, RefLabel, RefsOverview, RowKind, WorktreeInfo,
    };
    use ncrs_git_view::actions::{
        ChangeSide, Conflict, ConflictLine, Dialog, LocalChanges, MenuTarget, MergeInProgress,
        NewBranch, Notice, OpenMenu, Question, RenameBranch, StashEntry, SyncBar,
    };
    use ncrs_git_view::state::{Area, Detail, Subject};
    use ncrs_git_view::{update, view, Ctx, Message, State, ViewCtx};
    use ncrs_i18n::Language;
    use ncrs_ui_kit::format::DateFormat;
    use ncrs_ui_kit::git_rows::CommitColumns;
    use ncrs_ui_kit::palette::ThemePreset;
    use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

    const CTX: Ctx = Ctx::new(Language::German);
    const WINDOW: Size = Size::new(1740.0, 1130.0);
    const SUMMARIES: [&str; 10] = [
        "Rechnungen als PDF exportieren",
        "Rundung der Mehrwertsteuer auf Cent korrigiert",
        "Kassenbuch: Monatsabschluss ergänzt",
        "Fehler beim Import von Bankdateien mit Umlauten behoben",
        "Mahnwesen: dritte Mahnstufe hinzugefügt",
        "Kundenliste nach Ort und Name sortierbar",
        "Skonto bei Zahlung innerhalb von 14 Tagen",
        "Sicherung vor dem Löschen einer Buchung",
        "Dokumentation der Schnittstelle zum Steuerberater",
        "Abhängigkeiten aktualisiert",
    ];
    const AUTHORS: [&str; 3] = ["Lena Hartmann", "Tobias Engel", "Mira Sommer"];

    fn commit(number: usize) -> LogCommit {
        let spread = (number as u128 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c835);
        let id = format!("{spread:032x}{number:08x}");
        LogCommit {
            short: id.chars().take(7).collect(),
            id,
            parents: Vec::new(),
            author: AUTHORS.get(number % 3).expect("an author").to_string(),
            time: 1_790_000_000 - number as i64 * 7_300,
            summary: SUMMARIES
                .get(number % SUMMARIES.len())
                .expect("a summary")
                .to_string(),
            refs: Vec::new(),
            is_merge: false,
            graph: GraphRow::default(),
        }
    }

    fn branch(name: &str, is_current: bool, ahead: u32, behind: u32) -> BranchInfo {
        BranchInfo {
            name: name.to_owned(),
            is_current,
            has_upstream: true,
            ahead,
            behind,
            merged: false,
        }
    }

    fn label(name: &str, kind: RefKind) -> RefLabel {
        RefLabel {
            name: name.to_owned(),
            kind,
        }
    }

    fn file(path: &str, kind: ChangeKind, added: u32, removed: u32) -> ChangedFile {
        ChangedFile {
            path: path.to_owned(),
            old_path: None,
            kind,
            added,
            removed,
            binary: false,
        }
    }

    /// A repository of 40 commits with 7 changed files in the working copy.
    fn repository() -> State {
        let mut state = State {
            root: Some(PathBuf::from("/repo")),
            name: "kassenbuch".to_owned(),
            ..State::default()
        };
        let (read_log, _) = state.loads.log.begin();
        let mut commits: Vec<LogCommit> = (0..40).map(commit).collect();
        if let Some(first) = commits.first_mut() {
            first.refs = vec![
                label("main", RefKind::Branch),
                label("origin/main", RefKind::Remote),
            ];
        }
        if let Some(fourth) = commits.get_mut(3) {
            fourth.refs = vec![label("feature/rechnungs-export", RefKind::Branch)];
        }
        if let Some(tenth) = commits.get_mut(9) {
            tenth.refs = vec![label("v1.4.0", RefKind::Tag)];
        }
        let log = Log {
            commits,
            has_more: false,
            lane_count: 1,
            head: None,
        };
        drop(update(
            &mut state,
            Message::LogLoaded(read_log, Box::new(Ok(log))),
            &CTX,
        ));
        let (read_refs, _) = state.loads.refs.begin();
        let overview = RefsOverview {
            main_branch: Some("main".to_owned()),
            local: vec![
                branch("main", true, 1, 2),
                branch("feature/rechnungs-export", false, 3, 0),
                branch("feature/mehrwertsteuer", false, 0, 0),
                branch("fix/rundung", false, 0, 4),
                branch("release/1.4", false, 0, 0),
            ],
            remote: vec![
                "origin/main".to_owned(),
                "origin/feature/rechnungs-export".to_owned(),
                "origin/fix/rundung".to_owned(),
                "upstream/main".to_owned(),
            ],
            tags: vec![
                "v1.4.0".to_owned(),
                "v1.3.2".to_owned(),
                "v1.3.0".to_owned(),
            ],
            stashes: 3,
            worktrees: vec![WorktreeInfo {
                path: PathBuf::from("/repo"),
                branch: Some("main".to_owned()),
                head_short: None,
                wip: 7,
                ahead: 1,
                behind: 2,
                merged: false,
                is_main: true,
                is_current: true,
            }],
        };
        drop(update(
            &mut state,
            Message::RefsLoaded(read_refs, Box::new(Ok(overview))),
            &CTX,
        ));
        state.actions.stashes = vec![
            StashEntry {
                message: "WIP auf main: Mahnwesen umbauen".to_owned(),
                origin: "vor 2 Std.".to_owned(),
            },
            StashEntry {
                message: "WIP auf fix/rundung: Testfall".to_owned(),
                origin: "gestern".to_owned(),
            },
            StashEntry {
                message: "Skonto-Experiment".to_owned(),
                origin: "vor 6 Tagen".to_owned(),
            },
        ];
        state.actions.sync = Some(sync());
        state.actions.local = Some(local_changes());
        state
    }

    /// The second row of the list chosen, with its commit on the right.
    fn on_commit(state: &mut State) {
        state.commit_cursor = 0;
        let first = commit(0);
        state.detail = Some(Detail {
            subject: Subject::Commit(first.id.clone()),
            info: Some(CommitDetail {
                id: first.id,
                short: first.short,
                parents: Vec::new(),
                author: first.author,
                time: first.time,
                message: "Rechnungen als PDF exportieren\n\nDer Export nimmt die gewählten Rechnungen und legt sie als eine PDF-Datei je Kunde ab.\n\n- Vorlage mit Briefkopf aus den Einstellungen\n- Fehlende Anschrift wird vor dem Export gemeldet".to_owned(),
                files: Vec::new(),
            }),
            files: vec![
                file("src/export/pdf.rs", ChangeKind::Added, 212, 0),
                file("src/export/mod.rs", ChangeKind::Modified, 14, 2),
                file("src/ui/rechnungen.rs", ChangeKind::Modified, 31, 8),
            ],
        });
    }

    fn row(kind: RowKind, old: Option<u32>, new: Option<u32>, text: &str) -> DiffRow {
        DiffRow {
            kind,
            old_line: old,
            new_line: new,
            text: text.to_owned(),
        }
    }

    fn local_changes() -> LocalChanges {
        use RowKind::{Added, Context, Removed};
        let diff = FileDiff {
            path: "src/steuer/mwst.rs".to_owned(),
            hunks: vec![
                DiffHunk {
                    old_start: 41,
                    new_start: 41,
                    rows: vec![
                        row(
                            Context,
                            Some(41),
                            Some(41),
                            "pub fn brutto(netto: Cent, satz: Satz) -> Cent {",
                        ),
                        row(
                            Removed,
                            Some(42),
                            None,
                            "    let aufschlag = netto.0 * satz.prozent() / 100;",
                        ),
                        row(
                            Added,
                            None,
                            Some(42),
                            "    let aufschlag = (netto.0 * satz.prozent() + 50) / 100;",
                        ),
                        row(Context, Some(43), Some(43), "    Cent(netto.0 + aufschlag)"),
                        row(Context, Some(44), Some(44), "}"),
                    ],
                },
                DiffHunk {
                    old_start: 88,
                    new_start: 88,
                    rows: vec![
                        row(Context, Some(88), Some(88), "#[cfg(test)]"),
                        row(Context, Some(89), Some(89), "mod tests {"),
                        row(Added, None, Some(90), "    #[test]"),
                        row(Added, None, Some(91), "    fn rundet_halbe_cent_auf() {"),
                        row(
                            Added,
                            None,
                            Some(92),
                            "        assert_eq!(brutto(Cent(1_005), Satz::Voll), Cent(1_196));",
                        ),
                        row(Added, None, Some(93), "    }"),
                        row(Context, Some(90), Some(94), "}"),
                    ],
                },
            ],
            added: 5,
            removed: 1,
            binary: false,
            too_large: false,
        };
        LocalChanges {
            unstaged: vec![
                file("src/steuer/mwst.rs", ChangeKind::Modified, 5, 1),
                file("src/mahnwesen/stufen.rs", ChangeKind::Modified, 22, 4),
                file("src/mahnwesen/vorlagen/dritte_mahnung.txt", ChangeKind::Added, 18, 0),
                file("Cargo.lock", ChangeKind::Modified, 6, 6),
                file("doku/schnittstelle.md", ChangeKind::Deleted, 0, 41),
            ],
            staged: vec![
                file("src/kassenbuch/abschluss.rs", ChangeKind::Modified, 37, 3),
                file("src/kassenbuch/mod.rs", ChangeKind::Modified, 2, 0),
            ],
            selected: Some((ChangeSide::Unstaged, 0)),
            diff: Some(diff),
            title: "Mehrwertsteuer kaufmännisch runden".to_owned(),
            body: text_editor::Content::with_text(
                "Halbe Cent wurden bisher abgeschnitten. Jetzt wird wie in der Rechnung des Steuerberaters aufgerundet.",
            ),
            amend: false,
        }
    }

    fn merge() -> MergeInProgress {
        use ConflictLine::{Mine, Shared, Theirs};
        MergeInProgress {
            from: "feature/rechnungs-export".to_owned(),
            into: "main".to_owned(),
            conflicts: vec![
                Conflict {
                    path: "src/export/mod.rs".to_owned(),
                    places: 2,
                    edited: false,
                    resolved: false,
                },
                Conflict {
                    path: "src/ui/rechnungen.rs".to_owned(),
                    places: 1,
                    edited: false,
                    resolved: false,
                },
                Conflict {
                    path: "Cargo.toml".to_owned(),
                    places: 1,
                    edited: true,
                    resolved: false,
                },
                Conflict {
                    path: "README.md".to_owned(),
                    places: 1,
                    edited: false,
                    resolved: true,
                },
            ],
            selected: 0,
            preview: vec![
                Shared(12, "use crate::kassenbuch::Buchung;".to_owned()),
                Shared(13, "use crate::steuer::Cent;".to_owned()),
                Shared(14, String::new()),
                Mine("pub fn exportiere(buchungen: &[Buchung]) -> Result<Vec<u8>, Fehler> {".to_owned()),
                Mine("    let vorlage = Vorlage::laden(&einstellungen.briefkopf)?;".to_owned()),
                Theirs("pub fn exportiere(rechnungen: &[Rechnung], ziel: &Path) -> Result<(), Fehler> {".to_owned()),
                Theirs("    let vorlage = Vorlage::aus_datei(ziel.join(\"briefkopf.pdf\"))?;".to_owned()),
                Shared(17, "    let mut seiten = Vec::new();".to_owned()),
                Shared(18, "    for rechnung in rechnungen {".to_owned()),
                Shared(19, "        seiten.push(vorlage.seite(rechnung)?);".to_owned()),
                Shared(20, "    }".to_owned()),
                Shared(21, String::new()),
                Mine("    Ok(zusammenfuegen(seiten))".to_owned()),
                Theirs("    schreibe(ziel, zusammenfuegen(seiten))".to_owned()),
                Shared(24, "}".to_owned()),
            ],
        }
    }

    /// Over the first icon of the toolbar.
    const TOOLBAR_FETCH_X: f32 = 222.0;

    fn picture(state: &State, reference: &str) {
        picture_at(state, reference, None);
    }

    /// With the pointer over `hover`, so a tooltip shows.
    fn picture_at(state: &State, reference: &str, hover: Option<Point>) {
        let palette = ThemePreset::Cosmic.palette();
        let ctx = ViewCtx {
            lang: Language::German,
            palette: &palette,
            date_format: DateFormat::Iso,
            window: WINDOW,
            columns: CommitColumns::STANDARD,
        };
        let mut simulator = Simulator::with_size(
            iced::Settings {
                default_font: ncrs_ui_kit::APP_FONT,
                ..iced::Settings::default()
            },
            WINDOW,
            view(state, ctx).map(|message: Message| message),
        );
        if let Some(point) = hover {
            simulator.point_at(point);
            simulator.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: point,
            })]);
        }
        let matches = simulator
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(reference)
            .unwrap();
        assert!(matches, "{reference} differs from its image");
    }

    fn sync() -> SyncBar {
        SyncBar {
            ahead: 1,
            behind: 2,
            undo: Some("Commit „Fix Rundung der …“".to_owned()),
        }
    }

    #[test]
    fn local_changes_picture() {
        let mut state = repository();
        state.actions.changes_open = true;
        picture(&state, "tests/snapshots/git-local-changes-cosmic");
    }

    fn staged_diff() -> FileDiff {
        use RowKind::{Added, Context};
        FileDiff {
            path: "src/kassenbuch/abschluss.rs".to_owned(),
            hunks: vec![
                DiffHunk {
                    old_start: 10,
                    new_start: 10,
                    rows: vec![
                        row(
                            Context,
                            Some(10),
                            Some(10),
                            "pub fn monatsabschluss(monat: Monat) -> Abschluss {",
                        ),
                        row(
                            Added,
                            None,
                            Some(11),
                            "    let buchungen = buchungen_im(monat);",
                        ),
                        row(
                            Added,
                            None,
                            Some(12),
                            "    let summe = buchungen.iter().map(|b| b.betrag).sum();",
                        ),
                        row(
                            Context,
                            Some(11),
                            Some(13),
                            "    Abschluss::neu(monat, summe)",
                        ),
                    ],
                },
                DiffHunk {
                    old_start: 52,
                    new_start: 54,
                    rows: vec![
                        row(Context, Some(52), Some(54), "impl Abschluss {"),
                        row(
                            Added,
                            None,
                            Some(55),
                            "    pub fn ist_offen(&self) -> bool {",
                        ),
                        row(
                            Added,
                            None,
                            Some(56),
                            "        self.festgeschrieben.is_none()",
                        ),
                        row(Added, None, Some(57), "    }"),
                        row(Context, Some(53), Some(58), "}"),
                    ],
                },
            ],
            added: 6,
            removed: 0,
            binary: false,
            too_large: false,
        }
    }

    #[test]
    fn local_changes_staged_picture() {
        let mut state = repository();
        state.actions.sync = Some(sync());
        let mut local = local_changes();
        local.selected = Some((ChangeSide::Staged, 0));
        local.diff = Some(staged_diff());
        state.actions.local = Some(local);
        state.actions.changes_open = true;
        picture(&state, "tests/snapshots/git-local-changes-staged-cosmic");
    }

    #[test]
    fn toolbar_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        picture_at(
            &state,
            "tests/snapshots/git-toolbar-cosmic",
            Some(Point::new(TOOLBAR_FETCH_X, 16.0)),
        );
    }

    fn with_menu(target: MenuTarget, cursor: usize) -> State {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.area = Area::Sidebar;
        state.sidebar_cursor = cursor;
        state.actions.menu = Some(OpenMenu { target });
        state
    }

    #[test]
    fn local_branch_menu_picture() {
        let state = with_menu(MenuTarget::LocalBranch(1), 3);
        picture(&state, "tests/snapshots/git-branch-menu-local-cosmic");
    }

    #[test]
    fn remote_branch_menu_picture() {
        let state = with_menu(MenuTarget::RemoteBranch(1), 9);
        picture(&state, "tests/snapshots/git-branch-menu-remote-cosmic");
    }

    #[test]
    fn stash_menu_picture() {
        let state = with_menu(MenuTarget::Stash(0), 19);
        picture(&state, "tests/snapshots/git-stash-menu-cosmic");
    }

    #[test]
    fn new_branch_dialog_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.new_branch = Some(NewBranch {
            name: "feature/neue suche".to_owned(),
            from: "feature/rechnungs-export".to_owned(),
            checkout: true,
            problem: Some("Der Name darf keine Leerzeichen enthalten.".to_owned()),
        });
        picture(&state, "tests/snapshots/git-new-branch-dialog-cosmic");
    }

    fn with_dialog(dialog: Dialog) -> State {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.dialog = Some(dialog);
        state
    }

    #[test]
    fn discard_file_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DiscardFile {
            path: "src/rechnung/runden.rs".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-ask-discard-file-cosmic");
    }

    #[test]
    fn discard_all_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DiscardAll { count: 7 }));
        picture(&state, "tests/snapshots/git-ask-discard-all-cosmic");
    }

    #[test]
    fn delete_branch_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DeleteBranch {
            name: "feature/mehrwertsteuer".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-ask-delete-branch-cosmic");
    }

    #[test]
    fn delete_on_remote_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DeleteOnRemote {
            remote: "origin".to_owned(),
            branch: "fix/rundung".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-ask-delete-remote-cosmic");
    }

    #[test]
    fn drop_stash_question_picture() {
        let state = with_dialog(Dialog::Ask(Question::DropStash {
            sha: "0123456789abcdef".to_owned(),
            message: "Skonto-Experiment".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-ask-drop-stash-cosmic");
    }

    #[test]
    fn rename_dialog_picture() {
        let state = with_dialog(Dialog::Rename(RenameBranch {
            from: "feature/mehrwertsteuer".to_owned(),
            name: "feature/mehrwertsteuer-satz".to_owned(),
        }));
        picture(&state, "tests/snapshots/git-rename-dialog-cosmic");
    }

    #[test]
    fn merge_conflicts_picture() {
        let mut state = repository();
        state.actions.sync = Some(sync());
        state.actions.merge = Some(merge());
        picture(&state, "tests/snapshots/git-merge-conflicts-cosmic");
    }

    #[test]
    fn conflict_menu_picture() {
        let mut state = repository();
        state.actions.merge = Some(merge());
        state.actions.menu = Some(OpenMenu {
            target: MenuTarget::Conflict(2),
        });
        picture(&state, "tests/snapshots/git-conflict-menu-cosmic");
    }

    #[test]
    fn push_rejected_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.notice = Some(Notice::PushRejected {
            branch: "main".to_owned(),
            behind: 2,
        });
        picture(&state, "tests/snapshots/git-push-rejected-cosmic");
    }

    #[test]
    fn auth_failed_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.actions.notice = Some(Notice::AuthFailed {
            remote: "origin".to_owned(),
            output: "git@git.kassenbuch.example: Permission denied (publickey).\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.".to_owned(),
            details_open: true,
        });
        picture(&state, "tests/snapshots/git-auth-failed-cosmic");
    }

    #[test]
    fn stash_list_picture() {
        let mut state = repository();
        on_commit(&mut state);
        state.actions.sync = Some(sync());
        state.area = Area::Sidebar;
        state.sidebar_cursor = 18;
        picture(&state, "tests/snapshots/git-stash-list-cosmic");
    }
}
