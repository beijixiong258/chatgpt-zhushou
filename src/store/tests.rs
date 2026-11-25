use super::*;
use serde_json::json;

#[test]
fn interrupted_batch_is_restored_before_the_session_can_be_deleted_again() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path();
    let store = Store {
        home: root.to_owned(),
        state: root.join("state.sqlite"),
        trash: root.join("trash"),
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
        .trash
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

    let revision = store.open(&id).unwrap().revision;
    store.delete(&id, &revision).unwrap();
    assert!(!file.exists());
    assert!(store.open(&id).is_err());
    let batch = fs::read_dir(&store.trash)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("files.json").is_file())
        .unwrap();
    assert_eq!(
        fs::read_to_string(batch.join(file.file_name().unwrap())).unwrap(),
        content
    );
}
