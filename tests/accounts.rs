//! The account list file: round trips, a missing file and a corrupt one.

use stoat_gpui::accounts::{AccountList, AccountsFile, FileAccounts};

#[test]
fn list_round_trips_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    // A directory that doesn't exist yet, as on first launch.
    let file = FileAccounts::at(dir.path().join("config").join("accounts.json"));

    let mut list = AccountList::default();
    let a = list.add_or_update("https://api.stoat.chat", "01A", Some("Ann".into()));
    list.add_or_update("https://stoat.example.com/api", "01B", None);
    list.set_active(&a);
    file.save(&list).unwrap();

    assert_eq!(file.load().unwrap(), Some(list));
    // Only the list itself is left behind, no temporary file.
    let names: Vec<_> = std::fs::read_dir(dir.path().join("config"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["accounts.json"]);
}

#[test]
fn missing_file_loads_as_none() {
    let dir = tempfile::tempdir().unwrap();
    let file = FileAccounts::at(dir.path().join("accounts.json"));
    assert_eq!(file.load().unwrap(), None);
}

#[test]
fn an_emptied_list_is_not_a_first_launch() {
    let dir = tempfile::tempdir().unwrap();
    let file = FileAccounts::at(dir.path().join("accounts.json"));
    file.save(&AccountList::default()).unwrap();
    assert_eq!(file.load().unwrap(), Some(AccountList::default()));
}

#[test]
fn corrupt_file_is_an_error_and_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accounts.json");
    std::fs::write(&path, "{ not json").unwrap();
    let file = FileAccounts::at(&path);
    assert!(file.load().is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

#[test]
fn unknown_and_missing_fields_are_tolerated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accounts.json");
    std::fs::write(
        &path,
        r#"{"accounts":[{"id":"https://a","instance":"https://a","extra":1}]}"#,
    )
    .unwrap();
    let list = FileAccounts::at(&path).load().unwrap().unwrap();
    assert_eq!(list.accounts[0].user_id, None);
    assert_eq!(list.active, None);
}
