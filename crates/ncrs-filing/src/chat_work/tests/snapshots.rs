use super::*;

fn while_a_run_holds_the_index<T>(setup: &Setup, ask: impl FnOnce(&ChatAccess) -> T) -> T {
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let slot = Arc::clone(&setup.access.slot);
    let holder = std::thread::spawn(move || {
        let _guard = slot.lock().unwrap();
        locked_tx.send(()).unwrap();
        let _ = release_rx.recv();
    });
    locked_rx.recv().unwrap();
    let answer = ask(&setup.access);
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    answer
}

#[test]
fn the_ui_gets_no_answer_instead_of_waiting_while_a_run_holds_the_index() {
    let setup = Setup::new();

    assert!(while_a_run_holds_the_index(&setup, ChatAccess::try_kinds).is_err());
    assert!(setup.access.try_kinds().is_ok());
}

#[test]
fn the_ui_gets_the_last_known_kinds_while_a_run_holds_the_index() {
    let setup = Setup::new();
    let known = setup.access.try_kinds().unwrap();

    let busy = while_a_run_holds_the_index(&setup, ChatAccess::try_kinds);

    assert_eq!(busy, Ok(known));
}

#[test]
fn kinds_from_before_a_write_are_not_shown_while_the_index_is_busy() {
    let setup = Setup::new();
    setup.access.try_kinds().unwrap();

    setup.access.snapshots.forget();

    assert!(while_a_run_holds_the_index(&setup, ChatAccess::try_kinds).is_err());
}
