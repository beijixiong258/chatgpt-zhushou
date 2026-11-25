use super::{err, file_ids, restore, Result, Store};
use chrono::Local;
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub(super) struct Deletion {
    id: String,
    database: PathBuf,
    moves: Vec<(PathBuf, PathBuf)>,
}
impl Store {
    pub(super) fn prepare_deletion(
        &self,
        id: &str,
        files: &[PathBuf],
    ) -> Result<(PathBuf, Deletion)> {
        let dir = self.trash.join(format!(
            "chatgpt对话助手-{}-{}",
            Local::now().format("%Y%m%d-%H%M%S"),
            Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).map_err(err)?;
        let mut moves = vec![];
        for p in files {
            let from = self.checked_path(p)?;
            let target = dir.join(from.file_name().ok_or("历史文件名不完整")?);
            if moves.iter().any(|(_, t)| t == &target) {
                return Err("回收站目标文件重名".into());
            }
            moves.push((from, target));
        }
        let batch = Deletion {
            id: id.to_owned(),
            database: fs::canonicalize(&self.state).map_err(err)?,
            moves,
        };
        let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(err)?;
        serde_json::to_writer(&mut file, &batch).map_err(err)?;
        file.as_file().sync_all().map_err(err)?;
        file.persist_noclobber(dir.join("pending.json"))
            .map_err(err)?;
        Ok((dir, batch))
    }
    pub(super) fn execute_moves(&self, batch: &Deletion) -> Result<Vec<(PathBuf, PathBuf)>> {
        let mut done = vec![];
        for (from, to) in &batch.moves {
            if to.exists() {
                restore(&done)?;
                return Err("回收站已有同名文件".into());
            }
            if let Err(e) = fs::rename(from, to) {
                restore(&done)?;
                return Err(format!("移入回收站失败：{e}"));
            }
            done.push((from.clone(), to.clone()));
        }
        Ok(done)
    }
    pub(super) fn clear_projection(&self, files: &[PathBuf]) -> Result<()> {
        let path = self.state.with_file_name("thread_history_1.sqlite");
        if !path.is_file() {
            return Ok(());
        }
        let mut db =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(err)?;
        db.busy_timeout(std::time::Duration::from_secs(3))
            .map_err(err)?;
        let tx = db.transaction().map_err(err)?;
        for id in files
            .iter()
            .filter_map(|p| file_ids(p).map(|(_, r)| r))
            .collect::<HashSet<_>>()
        {
            for table in [
                "thread_items",
                "thread_turns",
                "thread_realtime_items",
                "thread_history_projection_state",
            ] {
                if tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
                        [table],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(err)?
                {
                    tx.execute(&format!("DELETE FROM {table} WHERE thread_id=?"), [&id])
                        .map_err(err)?;
                }
            }
        }
        tx.commit().map_err(err)
    }
    fn clear_name(&self, id: &str) -> Result<()> {
        let path = self.home.join("session_index.jsonl");
        if !path.exists() {
            return Ok(());
        }
        let gate = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.home.join("session_index.jsonl.lock"))
            .map_err(err)?;
        FileExt::try_lock_exclusive(&gate).map_err(|_| "会话名称索引正在写入".to_owned())?;
        let input = fs::read(&path).map_err(err)?;
        let mut bytes = Vec::with_capacity(input.len());
        for line in input.split_inclusive(|&x| x == b'\n') {
            if serde_json::from_slice::<Value>(line)
                .ok()
                .is_some_and(|v| v["id"] == id || v["thread_id"] == id)
            {
                continue;
            }
            bytes.extend_from_slice(line);
        }
        if bytes != input {
            let mut tmp = tempfile::NamedTempFile::new_in(&self.home).map_err(err)?;
            tmp.write_all(&bytes).map_err(err)?;
            tmp.as_file().sync_all().map_err(err)?;
            tmp.persist(&path).map_err(err)?;
        }
        Ok(())
    }
    pub(super) fn finish_deletion(&self, dir: &Path, batch: &Deletion) -> Result<()> {
        self.clear_projection(
            &batch
                .moves
                .iter()
                .map(|(p, _)| p.clone())
                .collect::<Vec<_>>(),
        )?;
        self.clear_name(&batch.id)?;
        fs::rename(dir.join("pending.json"), dir.join("files.json")).map_err(err)
    }
    pub(super) fn recover_deletions(&self) -> Result<()> {
        if !self.trash.is_dir() {
            return Ok(());
        }
        let state = fs::canonicalize(&self.state).map_err(err)?;
        for child in fs::read_dir(&self.trash).map_err(err)? {
            let child = child.map_err(err)?;
            let file_name = child.file_name();
            let name = file_name.to_string_lossy();
            if !child.file_type().map_err(err)?.is_dir()
                || !(name.starts_with("chatgpt对话助手-") || name.starts_with("CC-Switch-"))
            {
                continue;
            }
            let dir = child.path();
            let journal = dir.join("pending.json");
            if !journal.is_file() {
                continue;
            }
            let batch: Deletion =
                serde_json::from_reader(File::open(&journal).map_err(err)?).map_err(err)?;
            if batch.database != state {
                continue;
            }
            let Ok(_locks) = self.writer(&batch.id) else {
                continue;
            };
            if !journal.exists() {
                continue;
            }
            // Only recover direct files from this recycle operation to managed session roots.
            let roots: Vec<_> = ["sessions", "archived_sessions"]
                .iter()
                .filter_map(|s| fs::canonicalize(self.home.join(s)).ok())
                .collect();
            let mut moved = vec![];
            for (from, to) in &batch.moves {
                let parent = from.parent().ok_or("缺少会话目录")?;
                if !roots
                    .iter()
                    .any(|p| fs::canonicalize(parent).is_ok_and(|dir| dir.starts_with(p)))
                    || file_ids(from).is_none_or(|(id, _)| id != batch.id)
                    || to.parent() != Some(dir.as_path())
                    || to.file_name() != from.file_name()
                {
                    return Err("回收记录的路径不匹配".into());
                }
                if to.is_file() && !from.exists() {
                    moved.push((from.clone(), to.clone()));
                } else if to.exists() && from.exists() {
                    return Err("回收操作遇到重名文件，请保留两份文件".into());
                }
            }
            let db = self.db(false)?;
            let exists = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM threads WHERE id=?)",
                    [&batch.id],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(err)?;
            if exists {
                restore(&moved)?;
                fs::rename(journal, dir.join("cancelled.json")).map_err(err)?;
            } else {
                self.finish_deletion(&dir, &batch)?;
            }
        }
        Ok(())
    }
}
