use super::*;

/// The keys this binary ships with, in the order that decides which one a
/// label shows. Adding a file operation means adding a `KeyAction` and its
/// keys here; the key handler and the hint list follow from it.
///
/// Built at runtime rather than as a `static` because `Key::Character` needs a
/// `SmolStr`, which is not a const constructor.
pub fn default_bindings() -> Vec<(KeyAction, Binding)> {
    default_bindings_for(cfg!(target_os = "macos"))
}

/// The keys of one platform: `mac` picks the Mac's keys and takes Cmd where
/// the others take Ctrl, whatever this binary runs on. A test can check
/// both sets without a second build.
pub fn default_bindings_for(mac: bool) -> Vec<(KeyAction, Binding)> {
    // What `Modifiers::COMMAND` is on that platform.
    let command = if mac {
        Modifiers::LOGO
    } else {
        Modifiers::CTRL
    };
    use key::Named::*;
    use Key::{Character, Named as KeyNamed};
    use KeyAction as A;

    let mut bindings = vec![
        // --- Backspace, Enter, Tab and Cmd+Q are not on the function key bar ---
        // Not a bare Q: that letter belongs to type-ahead.
        (
            A::Quit,
            Binding::with_modifiers(Character("q".into()), command),
        ),
        (A::GoUp, Binding::key(KeyNamed(Backspace))),
        (A::Open, Binding::key(KeyNamed(Enter))),
        (A::SwitchPanel, Binding::key(KeyNamed(Tab))),
        // Tab (and Shift+Tab, which falls back to it) stays inside the shown
        // tab; Ctrl+Tab walks the open tabs: panels, then the pages. Ctrl, not
        // Cmd: Cmd+Tab is the application switcher on a Mac.
        (
            A::NextTab,
            Binding::with_modifiers(KeyNamed(Tab), Modifiers::CTRL),
        ),
        (
            A::PreviousTab,
            Binding::with_modifiers(KeyNamed(Tab), Modifiers::CTRL | Modifiers::SHIFT),
        ),
        // The arrows pick a panel when the panels have the keys; the text
        // fields keep them for the caret.
        (A::ActivateLeftPanel, Binding::key(KeyNamed(ArrowLeft))),
        (A::ActivateRightPanel, Binding::key(KeyNamed(ArrowRight))),
        (A::SwitchLanguage, Binding::key(KeyNamed(F9))),
        // --- selection: tagged rows ---
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
            A::TagAll,
            Binding::with_modifiers(Character("a".into()), command),
        ),
        // Total Commander's numpad keys; the dialog starts with `*.*`.
        (A::TagByPattern, Binding::key(Character("+".into()))),
        (A::UntagByPattern, Binding::key(Character("-".into()))),
        (
            A::GoToPath,
            Binding::with_modifiers(Character("g".into()), command),
        ),
        (
            A::ClearTags,
            Binding::with_modifiers(Character("*".into()), Modifiers::CTRL),
        ),
        // Cmd+C on a Mac, Ctrl+C elsewhere, as in the Finder and Explorer. A
        // Mac keeps Ctrl+C for stopping a job (see below); elsewhere Escape
        // stops it where free, else the cancel button in the progress line.
        (
            A::CopyFiles,
            Binding::with_modifiers(Character("c".into()), command),
        ),
        // Option+Cmd+C and Ctrl+Cmd+C on a Mac; Ctrl+Alt+C and Ctrl+Shift+C
        // elsewhere.
        (
            A::CopyPath,
            Binding::with_modifiers(Character("c".into()), Modifiers::ALT | command),
        ),
        (
            A::CopyName,
            Binding::with_modifiers(
                Character("c".into()),
                if mac {
                    Modifiers::CTRL | command
                } else {
                    Modifiers::CTRL | Modifiers::SHIFT
                },
            ),
        ),
        // The files, picture or text of the system clipboard go into the active
        // panel.
        (
            A::Paste,
            Binding::with_modifiers(Character("v".into()), command),
        ),
        (A::View, Binding::key(KeyNamed(F3))),
        (A::Edit, Binding::key(KeyNamed(F4))),
        // Cmd+Option+T (Ctrl+Alt+T elsewhere, the usual "open a terminal" key
        // of Linux desktops). Not advertised: the header has no room.
        (
            A::OpenTerminal,
            Binding::with_modifiers(Character("t".into()), Modifiers::ALT | command),
        ),
        (A::Copy, Binding::key(KeyNamed(F5))),
        (A::Move, Binding::key(KeyNamed(F6))),
        (
            A::Rename,
            Binding::with_modifiers(KeyNamed(F6), Modifiers::SHIFT),
        ),
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
            Binding::with_modifiers(Character("z".into()), command),
        ),
        (A::Mkdir, Binding::key(KeyNamed(F7))),
        // The classic Shift+F4: edit a new file.
        (
            A::NewFile,
            Binding::with_modifiers(KeyNamed(F4), Modifiers::SHIFT),
        ),
        // Alt+F5 packs into an archive, as in classic dual-panel file managers.
        (
            A::Pack,
            Binding::with_modifiers(KeyNamed(F5), Modifiers::ALT),
        ),
        // Alt (Option on macOS) with F1 or F2 picks the drive of the left or
        // right panel, as in classic dual-panel file managers. Not advertised: the bar shows
        // bare keys only.
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
            Binding::with_modifiers(Character("d".into()), command),
        ),
        (
            A::OpenFavourite1,
            Binding::with_modifiers(Character("1".into()), command),
        ),
        (
            A::OpenFavourite2,
            Binding::with_modifiers(Character("2".into()), command),
        ),
        (
            A::OpenFavourite3,
            Binding::with_modifiers(Character("3".into()), command),
        ),
        (
            A::OpenFavourite4,
            Binding::with_modifiers(Character("4".into()), command),
        ),
        (
            A::OpenFavourite5,
            Binding::with_modifiers(Character("5".into()), command),
        ),
        (
            A::OpenFavourite6,
            Binding::with_modifiers(Character("6".into()), command),
        ),
        (
            A::OpenFavourite7,
            Binding::with_modifiers(Character("7".into()), command),
        ),
        (
            A::OpenFavourite8,
            Binding::with_modifiers(Character("8".into()), command),
        ),
        (
            A::OpenFavourite9,
            Binding::with_modifiers(Character("9".into()), command),
        ),
        (A::SortByName, sort_binding_for(mac, SortColumn::Name)),
        (
            A::SortByModified,
            sort_binding_for(mac, SortColumn::Modified),
        ),
        (A::SortBySize, sort_binding_for(mac, SortColumn::Size)),
        // Shift+F10 is the context menu key on Windows and Linux. Registered so
        // the exact chord wins over the fall-back to bare F10, which quits.
        (
            A::ContextMenu,
            Binding::with_modifiers(KeyNamed(F10), Modifiers::SHIFT),
        ),
        // The menu key of a PC keyboard does what Shift+F10 does.
        (A::ContextMenu, Binding::key(KeyNamed(ContextMenu))),
        // Quit is also F10, the classic dual-panel convention.
        (A::Quit, Binding::key(KeyNamed(F10))),
        // --- bound, not advertised: the header has no room for these ---
        (A::CursorUp, Binding::key(KeyNamed(ArrowUp))),
        (A::CursorDown, Binding::key(KeyNamed(ArrowDown))),
        (A::PageUp, Binding::key(KeyNamed(PageUp))),
        (A::PageDown, Binding::key(KeyNamed(PageDown))),
        // Cmd+Up/Down page on a Mac, where there is no PageUp key; `command`
        // is Ctrl elsewhere.
        (
            A::PageUp,
            Binding::with_modifiers(KeyNamed(ArrowUp), command),
        ),
        (
            A::PageDown,
            Binding::with_modifiers(KeyNamed(ArrowDown), command),
        ),
        (A::First, Binding::key(KeyNamed(Home))),
        (A::Last, Binding::key(KeyNamed(End))),
        // Alt+F7 is the classic file search key. Cmd+Shift+F (Ctrl+Shift+F
        // elsewhere) is VS Code's search in files.
        (
            A::Search,
            Binding::with_modifiers(KeyNamed(F7), Modifiers::ALT),
        ),
        (
            A::Search,
            Binding::with_modifiers(Character("f".into()), command | Modifiers::SHIFT),
        ),
        // VS Code's Go to File.
        (
            A::QuickOpen,
            Binding::with_modifiers(Character("p".into()), command),
        ),
        // F2 was the classic user menu, the key that ran user commands;
        // here it asks Hermes for help. Alt+F2 stays the right drive menu, the
        // command line is on Shift+F2.
        (A::HermesChat, Binding::key(KeyNamed(F2))),
        (
            A::CommandLine,
            Binding::with_modifiers(KeyNamed(F2), Modifiers::SHIFT),
        ),
        // VS Code's Command Palette; F1 opens it there too.
        (
            A::CommandPalette,
            Binding::with_modifiers(Character("p".into()), command | Modifiers::SHIFT),
        ),
        (A::CommandPalette, Binding::key(KeyNamed(F1))),
        // The settings tab; Cmd+, (Ctrl+, elsewhere) is the usual key for preferences.
        (
            A::Settings,
            Binding::with_modifiers(Character(",".into()), command),
        ),
        // The System page.
        (A::SystemMonitor, Binding::key(KeyNamed(F12))),
        // The keyboard shortcuts, K for keys; Cmd+K, Ctrl+K elsewhere.
        (
            A::KeyBindings,
            Binding::with_modifiers(Character("k".into()), command),
        ),
        // The filing status page; Cmd+Shift+D, Ctrl+Shift+D elsewhere.
        (
            A::FilingStatus,
            Binding::with_modifiers(Character("d".into()), command | Modifiers::SHIFT),
        ),
        // The rules, B for bureaucratic regulations; Cmd+Shift+B, Ctrl+Shift+B elsewhere.
        (
            A::Rules,
            Binding::with_modifiers(Character("b".into()), command | Modifiers::SHIFT),
        ),
        // Process the file under the cursor again, H for Hermes; Cmd+Shift+H,
        // Ctrl+Shift+H elsewhere.
        (
            A::HermesProcessAgain,
            Binding::with_modifiers(Character("h".into()), command | Modifiers::SHIFT),
        ),
        // The note on the file under the cursor, N for note; Cmd+Shift+N,
        // Ctrl+Shift+N elsewhere.
        (
            A::NoteOnFile,
            Binding::with_modifiers(Character("n".into()), command | Modifiers::SHIFT),
        ),
        // The saved queries, A for abfragen; Cmd+Shift+A, Ctrl+Shift+A elsewhere.
        (
            A::SavedQueries,
            Binding::with_modifiers(Character("a".into()), command | Modifiers::SHIFT),
        ),
        // The Git view of the repository the panel is in, G for Git;
        // Cmd+Shift+G, Ctrl+Shift+G elsewhere.
        (
            A::GitView,
            Binding::with_modifiers(Character("g".into()), command | Modifiers::SHIFT),
        ),
        // The history and the blame of the file under the cursor; the Git tab
        // has the same keys for the file it shows.
        (
            A::GitFileHistory,
            Binding::with_modifiers(Character("y".into()), command),
        ),
        (
            A::GitFileBlame,
            Binding::with_modifiers(Character("i".into()), command),
        ),
        // The list of all repositories, K for the keep of projects;
        // Cmd+Shift+K, Ctrl+Shift+K elsewhere.
        (
            A::Repos,
            Binding::with_modifiers(Character("k".into()), command | Modifiers::SHIFT),
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
    if mac {
        bindings.push((
            A::AbortJob,
            Binding::with_modifiers(Character("c".into()), Modifiers::CTRL),
        ));
    }
    // Option+Cmd+R on a Mac; Ctrl+Alt+R elsewhere, where bare Alt+R is already
    // the regex toggle of the search.
    bindings.push((
        A::Permissions,
        Binding::with_modifiers(
            Character("r".into()),
            if mac {
                Modifiers::ALT | command
            } else {
                Modifiers::CTRL | Modifiers::ALT
            },
        ),
    ));
    if mac {
        let (held, key) = mac_menu_chord();
        bindings.insert(0, (A::ContextMenu, Binding::with_modifiers(key, held)));
    }
    // Safari's and Finder's back and forward; elsewhere Alt+arrows are enough.
    if mac {
        bindings.push((
            A::HistoryBack,
            Binding::with_modifiers(Character("[".into()), command),
        ));
        bindings.push((
            A::HistoryForward,
            Binding::with_modifiers(Character("]".into()), command),
        ));
    }
    bindings
}

/// Ctrl+F3, Ctrl+F5 and Ctrl+F6 as in classic dual-panel file managers. A Mac takes Ctrl+F-keys
/// for itself, so there it is Option+Cmd+1 (name), Option+Cmd+2 (size) and Option+Cmd+3
/// (modified); plain Cmd+1 to Cmd+9 open the first nine favourites.
pub fn sort_binding(column: SortColumn) -> Binding {
    sort_binding_for(cfg!(target_os = "macos"), column)
}

fn sort_binding_for(mac: bool, column: SortColumn) -> Binding {
    use key::Named::{F3, F5, F6};
    if mac {
        let digit = match column {
            SortColumn::Name => "1",
            SortColumn::Size => "2",
            SortColumn::Modified => "3",
        };
        Binding::with_modifiers(
            Key::Character(digit.into()),
            Modifiers::ALT | Modifiers::LOGO,
        )
    } else {
        let function_key = match column {
            SortColumn::Name => F3,
            SortColumn::Modified => F5,
            SortColumn::Size => F6,
        };
        Binding::with_modifiers(Key::Named(function_key), Modifiers::CTRL)
    }
}
