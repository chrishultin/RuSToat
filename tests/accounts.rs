//! The account list file: round trips, a missing file and a corrupt one,
//! and a reordered list.

use stoat_gpui::accounts::{AccountList, AccountsFile, FileAccounts, MemoryAccounts};

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

#[test]
fn a_file_from_before_session_ids_still_loads_and_they_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accounts.json");
    std::fs::write(
        &path,
        r#"{"accounts":[{"id":"a1","instance":"https://api.stoat.chat","user_id":"01A","name":"Ann"}],"active":"a1"}"#,
    )
    .unwrap();
    let file = FileAccounts::at(path);
    let mut list = file.load().unwrap().unwrap();
    assert_eq!(list.get("a1").unwrap().session_id, None);

    assert!(list.set_session("a1", "01SESSION"));
    file.save(&list).unwrap();
    let again = file.load().unwrap().unwrap();
    assert_eq!(
        again.get("a1").unwrap().session_id.as_deref(),
        Some("01SESSION")
    );
}

fn ids(list: &AccountList) -> Vec<String> {
    list.accounts.iter().map(|a| a.id.clone()).collect()
}

#[test]
fn a_reordered_list_keeps_its_order() {
    let mut list = AccountList::default();
    let a = list.add_or_update("https://api.stoat.chat", "01A", Some("Ann".into()));
    let b = list.add_or_update("https://api.stoat.chat", "01B", Some("Bo".into()));
    let c = list.add_or_update("https://stoat.example.com/api", "01C", None);
    list.set_active(&b);
    // Dragging the third account to the top.
    assert!(list.move_account(&c, Some(&a)));

    let memory = MemoryAccounts::default();
    memory.save(&list).unwrap();
    let loaded = memory.load().unwrap().unwrap();
    assert_eq!(ids(&loaded), [c.clone(), a.clone(), b.clone()]);
    assert_eq!(loaded.active, Some(b.clone()));

    // And through the file, as across a restart.
    let dir = tempfile::tempdir().unwrap();
    let file = FileAccounts::at(dir.path().join("accounts.json"));
    file.save(&loaded).unwrap();
    assert_eq!(ids(&file.load().unwrap().unwrap()), [c, a, b]);
}
