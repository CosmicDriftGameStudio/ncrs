use super::*;

/// The keys this binary ships with, in the order that decides which one a
/// label shows. Adding a file operation means adding a `KeyAction` and its
/// keys here; the key handler and the hint list follow from it.
///
/// Built at runtime rather than as a `static` because `Key::Character` needs a
/// `SmolStr`, which is not a const constructor.
pub fn default_bindings() -> Vec<(KeyAction, Binding)> {
    use key::Named::*;
    use Key::{Character, Named as KeyNamed};
    use KeyAction as A;

    let mut bindings = vec![
        // --- Backspace, Enter, Tab and Cmd+Q are not on the function key bar ---
        // Not a bare Q: that letter belongs to type-ahead.
        (
            A::Quit,
            Binding::with_modifiers(Character("q".into()), Modifiers::COMMAND),
        ),
        (A::GoUp, Binding::key(KeyNamed(Backspace))),
        (A::Open, Binding::key(KeyNamed(Enter))),
        (A::SwitchPanel, Binding::key(KeyNamed(Tab))),
        (A::SwitchLanguage, Binding::key(KeyNamed(F9))),
        // --- selection: tagged rows, not advertised ---
        (A::ToggleTag, Binding::key(KeyNamed(Insert))),
        (A::ToggleTag, Binding::key(KeyNamed(Space))),
        (
            A::TagDown,
            Binding::with_modifiers(KeyNamed(ArrowDown), Modifiers::SHIFT),
        ),
        (
            A::TagUp,
            Binding::with_modifiers(KeyNamed(ArrowUp), Modifiers::SHIFT),
        ),
        (A::TagAll, Binding::key(Character("*".into()))),
        (
            A::ClearTags,
            Binding::with_modifiers(Character("*".into()), Modifiers::CTRL),
        ),
        // Cmd+C on a Mac, Ctrl+C elsewhere, as in the Finder and Explorer. A
        // Mac keeps Ctrl+C for stopping a job (see below); elsewhere Escape
        // stops it where free, else the cancel button in the progress line.
        (
            A::CopyFiles,
            Binding::with_modifiers(Character("c".into()), Modifiers::COMMAND),
        ),
        // Option+Cmd+C and Ctrl+Cmd+C on a Mac; Ctrl+Alt+C and Ctrl+Shift+C
        // elsewhere.
        (
            A::CopyPath,
            Binding::with_modifiers(Character("c".into()), Modifiers::ALT | Modifiers::COMMAND),
        ),
        (
            A::CopyName,
            Binding::with_modifiers(
                Character("c".into()),
                if cfg!(target_os = "macos") {
                    Modifiers::CTRL | Modifiers::COMMAND
                } else {
                    Modifiers::CTRL | Modifiers::SHIFT
                },
            ),
        ),
        // The files, picture or text of the system clipboard go into the active
        // panel.
        (
            A::Paste,
            Binding::with_modifiers(Character("v".into()), Modifiers::COMMAND),
        ),
        (A::View, Binding::key(KeyNamed(F3))),
        (A::Edit, Binding::key(KeyNamed(F4))),
        (A::Copy, Binding::key(KeyNamed(F5))),
        (A::Move, Binding::key(KeyNamed(F6))),
        (A::Delete, Binding::key(KeyNamed(F8))),
        // Not advertised: the header has no room, and a key that deletes for
        // good should be found in the help, not stumbled over.
        (
            A::DeletePermanently,
            Binding::with_modifiers(KeyNamed(F8), Modifiers::SHIFT),
        ),
        // Cmd+Z (Ctrl+Z elsewhere) puts back what F8 just moved to the trash,
        // as the Finder and Explorer undo a delete.
        (
            A::RestoreLastDeletion,
            Binding::with_modifiers(Character("z".into()), Modifiers::COMMAND),
        ),
        (A::Mkdir, Binding::key(KeyNamed(F7))),
        // Norton Commander's Shift+F4: edit a new file.
        (
            A::NewFile,
            Binding::with_modifiers(KeyNamed(F4), Modifiers::SHIFT),
        ),
        // Alt+F5 packs into an archive, as in Norton Commander.
        (
            A::Pack,
            Binding::with_modifiers(KeyNamed(F5), Modifiers::ALT),
        ),
        // Alt (Option on macOS) with F1 or F2 picks the drive of the left or
        // right panel, as in Norton Commander. Not advertised: the bar shows
        // bare keys only, and NC does not list these either.
        (
            A::DriveMenuLeft,
            Binding::with_modifiers(KeyNamed(F1), Modifiers::ALT),
        ),
        (
            A::DriveMenuRight,
            Binding::with_modifiers(KeyNamed(F2), Modifiers::ALT),
        ),
        // Alt+Left and Alt+Right walk the panel's folders back and forward, as
        // in a browser.
        (
            A::HistoryBack,
            Binding::with_modifiers(KeyNamed(ArrowLeft), Modifiers::ALT),
        ),
        (
            A::HistoryForward,
            Binding::with_modifiers(KeyNamed(ArrowRight), Modifiers::ALT),
        ),
        // Cmd+D (Ctrl+D elsewhere) bookmarks the folder, as in a browser.
        (
            A::AddFavourite,
            Binding::with_modifiers(Character("d".into()), Modifiers::COMMAND),
        ),
        (A::SortByName, sort_binding(SortColumn::Name)),
        (A::SortByModified, sort_binding(SortColumn::Modified)),
        (A::SortBySize, sort_binding(SortColumn::Size)),
        // Shift+F10 is the context menu key on Windows and Linux. Registered so
        // the exact chord wins over the fall-back to bare F10, which quits.
        (
            A::ContextMenu,
            Binding::with_modifiers(KeyNamed(F10), Modifiers::SHIFT),
        ),
        // Quit is also F10, the Norton Commander convention.
        (A::Quit, Binding::key(KeyNamed(F10))),
        // --- bound, not advertised: the header has no room for these ---
        (A::CursorUp, Binding::key(KeyNamed(ArrowUp))),
        (A::CursorDown, Binding::key(KeyNamed(ArrowDown))),
        (A::PageUp, Binding::key(KeyNamed(PageUp))),
        (A::PageDown, Binding::key(KeyNamed(PageDown))),
        // Cmd+Up/Down page on a Mac, where there is no PageUp key; `COMMAND`
        // is Ctrl elsewhere.
        (
            A::PageUp,
            Binding::with_modifiers(KeyNamed(ArrowUp), Modifiers::COMMAND),
        ),
        (
            A::PageDown,
            Binding::with_modifiers(KeyNamed(ArrowDown), Modifiers::COMMAND),
        ),
        (A::First, Binding::key(KeyNamed(Home))),
        (A::Last, Binding::key(KeyNamed(End))),
        // Alt+F7 is Norton Commander's file search. Cmd+Shift+F (Ctrl+Shift+F
        // elsewhere) is VS Code's search in files.
        (
            A::Search,
            Binding::with_modifiers(KeyNamed(F7), Modifiers::ALT),
        ),
        (
            A::Search,
            Binding::with_modifiers(Character("f".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // VS Code's Go to File.
        (
            A::QuickOpen,
            Binding::with_modifiers(Character("p".into()), Modifiers::COMMAND),
        ),
        // F2 was Norton Commander's user menu, the key that ran user commands;
        // Alt+F2 stays the right drive menu.
        (A::CommandLine, Binding::key(KeyNamed(F2))),
        // VS Code's Command Palette; F1 opens it there too.
        (
            A::CommandPalette,
            Binding::with_modifiers(Character("p".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        (A::CommandPalette, Binding::key(KeyNamed(F1))),
        // The settings tab; Cmd+, (Ctrl+, elsewhere) is the usual key for preferences.
        (
            A::Settings,
            Binding::with_modifiers(Character(",".into()), Modifiers::COMMAND),
        ),
        // The keyboard shortcuts, K for keys; Cmd+K, Ctrl+K elsewhere.
        (
            A::KeyBindings,
            Binding::with_modifiers(Character("k".into()), Modifiers::COMMAND),
        ),
        // The filing status page; Cmd+Shift+D, Ctrl+Shift+D elsewhere.
        (
            A::FilingStatus,
            Binding::with_modifiers(Character("d".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The filing setup, L for locations; Cmd+Shift+L, Ctrl+Shift+L elsewhere.
        (
            A::FilingSetup,
            Binding::with_modifiers(Character("l".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The rules, B for bureaucratic regulations; Cmd+Shift+B, Ctrl+Shift+B elsewhere.
        (
            A::Rules,
            Binding::with_modifiers(Character("b".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The rule suggestions, R for requisitions; Cmd+Shift+R, Ctrl+Shift+R elsewhere.
        (
            A::RuleSuggestions,
            Binding::with_modifiers(Character("r".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The review list, V for verify; Cmd+Shift+V, Ctrl+Shift+V elsewhere.
        (
            A::ReviewList,
            Binding::with_modifiers(Character("v".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The quick fixes of an inquiry, as in VS Code and Xcode; Cmd+. (Ctrl+. elsewhere).
        (
            A::HermesQuickFix,
            Binding::with_modifiers(Character(".".into()), Modifiers::COMMAND),
        ),
        // Process the file under the cursor again, H for Hermes; Cmd+Shift+H,
        // Ctrl+Shift+H elsewhere. Cmd+Shift+R already opens the rule suggestions.
        (
            A::HermesProcessAgain,
            Binding::with_modifiers(Character("h".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The saved queries, A for abfragen; Cmd+Shift+A, Ctrl+Shift+A elsewhere.
        (
            A::SavedQueries,
            Binding::with_modifiers(Character("a".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The reorganisation plan, I for inventory; Cmd+Shift+I, Ctrl+Shift+I elsewhere.
        (
            A::ReorganisationPlan,
            Binding::with_modifiers(Character("i".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // Undo of the last reorganisation, U for undo; Cmd+Shift+U, Ctrl+Shift+U elsewhere.
        (
            A::UndoReorganisation,
            Binding::with_modifiers(Character("u".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The toggles of the search fields, as in VS Code. On a Mac Option types
        // other characters, so these are only found through the physical key.
        (
            A::SearchCase,
            Binding::with_modifiers(Character("c".into()), Modifiers::ALT),
        ),
        (
            A::SearchWholeWord,
            Binding::with_modifiers(Character("w".into()), Modifiers::ALT),
        ),
        (
            A::SearchRegex,
            Binding::with_modifiers(Character("r".into()), Modifiers::ALT),
        ),
    ];
    // Ctrl is not Cmd on a Mac, so Ctrl+C can keep stopping a job there;
    // elsewhere Ctrl+C is copy_files.
    if cfg!(target_os = "macos") {
        bindings.push((
            A::AbortJob,
            Binding::with_modifiers(Character("c".into()), Modifiers::CTRL),
        ));
    }
    // Safari's and Finder's back and forward; elsewhere Alt+arrows are enough.
    if cfg!(target_os = "macos") {
        bindings.push((
            A::HistoryBack,
            Binding::with_modifiers(Character("[".into()), Modifiers::COMMAND),
        ));
        bindings.push((
            A::HistoryForward,
            Binding::with_modifiers(Character("]".into()), Modifiers::COMMAND),
        ));
    }
    bindings
}

/// Ctrl+F3, Ctrl+F5 and Ctrl+F6 as in Norton Commander. A Mac takes Ctrl+F-keys
/// for itself, so there it is Cmd+1 (name), Cmd+2 (size) and Cmd+3 (modified).
pub fn sort_binding(column: SortColumn) -> Binding {
    use key::Named::{F3, F5, F6};
    if cfg!(target_os = "macos") {
        let digit = match column {
            SortColumn::Name => "1",
            SortColumn::Size => "2",
            SortColumn::Modified => "3",
        };
        Binding::with_modifiers(Key::Character(digit.into()), Modifiers::COMMAND)
    } else {
        let function_key = match column {
            SortColumn::Name => F3,
            SortColumn::Modified => F5,
            SortColumn::Size => F6,
        };
        Binding::with_modifiers(Key::Named(function_key), Modifiers::CTRL)
    }
}
