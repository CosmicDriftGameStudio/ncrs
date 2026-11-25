use super::*;

#[test]
fn filing_continuously_is_written_and_read_back_and_is_off_by_default() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "version = 1\n").unwrap();
    let (inbox, other, archive) = (
        base.path().join("inbox"),
        base.path().join("other"),
        base.path().join("archive"),
    );
    for folder in [&inbox, &other, &archive] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let locations = [
        FilingLocation {
            continuous: true,
            transfer: LocationTransfer::Copy,
            ..chosen(&inbox, LocationMode::Inbox, true, false)
        },
        chosen(&other, LocationMode::Inbox, true, false),
        chosen(&archive, LocationMode::Archive, false, true),
    ];

    let config = write_read_folders_with_home(&path, &locations, None).unwrap();

    let read: Vec<_> = config
        .filing
        .locations
        .iter()
        .map(|location| (location.continuous, location.transfer))
        .collect();
    assert_eq!(
        read,
        [
            (true, LocationTransfer::Copy),
            (false, LocationTransfer::Move),
            (false, LocationTransfer::Move)
        ]
    );
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap()
            .matches("continuous")
            .count(),
        1
    );
}
