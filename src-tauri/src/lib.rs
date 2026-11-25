mod items;
mod store;

use items::Change;
use store::{Document, Session, Store};

#[tauri::command]
async fn history_list() -> Result<Vec<Session>, String> {
    tauri::async_runtime::spawn_blocking(|| Store::local()?.list())
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn history_open(id: String) -> Result<Document, String> {
    tauri::async_runtime::spawn_blocking(move || Store::local()?.open(&id))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn history_save(
    id: String,
    revision: String,
    changes: Vec<Change>,
) -> Result<Document, String> {
    tauri::async_runtime::spawn_blocking(move || Store::local()?.save(&id, &revision, &changes))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn history_delete(id: String, revision: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || Store::local()?.delete(&id, &revision))
        .await
        .map_err(|e| e.to_string())?
}
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            history_list,
            history_open,
            history_save,
            history_delete
        ])
        .run(tauri::generate_context!())
        .expect("无法启动chatgpt对话助手");
}
