use crate::{items::Change, store::Store};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum Operation {
    HistoryList,
    HistoryOpen {
        id: String,
    },
    HistorySave {
        id: String,
        revision: String,
        changes: Vec<Change>,
    },
    HistoryUnarchive {
        id: String,
        revision: String,
    },
    HistoryDelete {
        id: String,
        revision: String,
    },
}

#[derive(Deserialize)]
pub struct Request {
    sequence: u64,
    #[serde(flatten)]
    operation: Operation,
}

impl Request {
    pub fn respond(self) -> String {
        let result = Store::local().and_then(|store| match self.operation {
            Operation::HistoryList => Ok(json!(store.list()?)),
            Operation::HistoryOpen { id } => Ok(json!(store.open(&id)?)),
            Operation::HistorySave {
                id,
                revision,
                changes,
            } => Ok(json!(store.save(&id, &revision, &changes)?)),
            Operation::HistoryUnarchive { id, revision } => {
                store.unarchive(&id, &revision)?;
                Ok(Value::Null)
            }
            Operation::HistoryDelete { id, revision } => {
                store.delete(&id, &revision)?;
                Ok(Value::Null)
            }
        });
        match result {
            Ok(value) => json!({"sequence": self.sequence, "value": value}),
            Err(error) => json!({"sequence": self.sequence, "error": error}),
        }
        .to_string()
    }
}
