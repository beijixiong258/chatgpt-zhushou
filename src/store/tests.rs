use super::*;
use serde_json::json;

#[test]
fn interrupted_batch_is_restored_before_the_session_can_be_deleted_again() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path();
    let store = Store {
        home: root.to_owned(),
        state: root.join("state.sqlite"),
        legacy_trash: Some(root.join("trash")),
    };
    let sessions = root.join("archived_sessions");
    fs::create_dir(&sessions).unwrap();
    let id = Uuid::new_v4().to_string();
    let file = sessions.join(format!("rollout-2026-11-25T12-00-00-{id}.jsonl"));
    let content = format!("{}\n", json!({"type":"session_meta","payload":{"id":id}}));
    fs::write(&file, &content).unwrap();
    let db = Connection::open(&store.state).unwrap();
    db.execute_batch(
        "CREATE TABLE threads(id TEXT PRIMARY KEY, rollout_path TEXT, archived INTEGER NOT NULL)",
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES(?,?,1)",
        params![id, file.to_str().unwrap()],
    )
    .unwrap();

    // Simulate the interruption after moving a file but before committing the index change.
    let dir = store
        .legacy_trash
        .as_ref()
        .unwrap()
        .join(format!("previous-name-20261125-120000-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let moved = dir.join(file.file_name().unwrap());
    let from = fs::canonicalize(&file).unwrap();
    let journal =
        json!({"id":id,"database":fs::canonicalize(&store.state).unwrap(),"moves":[[from,moved]]});
    fs::write(dir.join("pending.json"), journal.to_string()).unwrap();
    fs::rename(&file, &moved).unwrap();
    store.recover_deletions().unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), content);
    assert!(dir.join("cancelled.json").is_file());

    // The new journal also restores a staged file if the index still exists.
    let pointer = file.to_str().unwrap();
    let (pending, batch) = store
        .prepare_deletion(&id, pointer, std::slice::from_ref(&file))
        .unwrap();
    store.execute_moves(&batch).unwrap();
    assert!(!file.exists());
    store.recover_deletions().unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), content);
    assert!(!pending.exists());

    let revision = store.open(&id).unwrap().revision;
    store.delete(&id, &revision).unwrap();
    assert!(!file.exists());
    assert!(store.open(&id).is_err());
    assert!(fs::read_dir(&sessions).unwrap().next().is_none());
    assert!(fs::read_dir(store.deletion_dir()).unwrap().next().is_none());
    assert_eq!(
        fs::read_dir(store.legacy_trash.as_ref().unwrap())
            .unwrap()
            .count(),
        1
    );
}
