use crate::items::{self, Change, Entry};
use chrono::Local;
use fs2::FileExt;
use rusqlite::{params, Connection, OpenFlags, Transaction, TransactionBehavior};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

mod recycle;
#[cfg(test)]
mod tests;
mod unarchive;

type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[derive(Serialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub project: String,
    pub project_name: String,
    pub cwd: String,
    pub preview: String,
    pub updated: i64,
}
#[derive(Serialize)]
pub struct Document {
    pub revision: String,
    pub entries: Vec<Entry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}
pub struct Store {
    home: PathBuf,
    state: PathBuf,
    trash: PathBuf,
}
impl Store {
    pub fn local() -> Result<Self> {
        let user = dirs::home_dir().ok_or("无法定位用户目录")?;
        let home = std::env::var_os("CODEX_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| user.join(".codex"));
        let configured = fs::read_to_string(home.join("config.toml"))
            .ok()
            .and_then(|s| s.parse::<toml::Value>().ok())
            .and_then(|v| v.get("sqlite_home")?.as_str().map(PathBuf::from));
        let state = std::env::var_os("CODEX_SQLITE_HOME")
            .map(PathBuf::from)
            .or(configured)
            .map(|p| if p.is_absolute() { p } else { home.join(p) })
            .unwrap_or_else(|| home.clone())
            .join("state_5.sqlite");
        let trash = dirs::desktop_dir()
            .ok_or("无法定位桌面")?
            .join("智能体回收站")
            .join("垃圾桶");
        Ok(Self { home, state, trash })
    }
    fn db(&self, write: bool) -> Result<Connection> {
        Connection::open_with_flags(
            &self.state,
            if write {
                OpenFlags::SQLITE_OPEN_READ_WRITE
            } else {
                OpenFlags::SQLITE_OPEN_READ_ONLY
            },
        )
        .and_then(|db| {
            db.busy_timeout(std::time::Duration::from_secs(3))?;
            db.pragma_update(None, "foreign_keys", "ON")?;
            Ok(db)
        })
        .map_err(err)
    }
    pub fn list(&self) -> Result<Vec<Session>> {
        if !self.state.is_file() {
            return Ok(vec![]);
        }
        self.recover_deletions()?;
        let db = self.db(false)?;
        let projects = projects(&db)?;
        let has_project_id: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('threads') WHERE name='project_id')",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        let project_id = if has_project_id { "project_id" } else { "NULL" };
        let mut q = db
            .prepare(&format!(
                "SELECT id, coalesce(nullif(name,''),nullif(title,''),'未命名会话'),
                        cwd, coalesce(preview,''), updated_at, {project_id}, rollout_path
                 FROM threads WHERE archived = 1 ORDER BY updated_at DESC, id"
            ))
            .map_err(err)?;
        let indexed = q
            .query_map([], |r| {
                let raw: Option<String> = r.get(2)?;
                let cwd = plain_path(Path::new(raw.as_deref().unwrap_or("")));
                let id: Option<String> = r.get(5)?;
                let (project, project_name) = project_for(id.as_deref(), &cwd, &projects);
                let session = Session {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    project,
                    project_name,
                    cwd,
                    preview: r.get(3)?,
                    updated: r.get(4)?,
                };
                Ok((session, r.get::<_, String>(6)?))
            })
            .map_err(err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)?;
        drop(q);
        drop(db);

        let mut sessions = Vec::new();
        for (session, pointer) in indexed {
            if matches!(rollout_file(Path::new(&pointer)), Ok(None)) {
                if let Err(e) = self.prune_missing(&session.id, &pointer) {
                    // A later refresh can retry without showing the missing session.
                    eprintln!("无法清理缺失的会话 {}：{e}", session.id);
                }
            } else {
                sessions.push(session);
            }
        }
        Ok(sessions)
    }
    fn checked_path(&self, p: &Path) -> Result<PathBuf> {
        let p = fs::canonicalize(p).map_err(err)?;
        let ok = ["sessions", "archived_sessions"]
            .iter()
            .filter_map(|s| fs::canonicalize(self.home.join(s)).ok())
            .any(|root| p.starts_with(root));
        if !ok || file_ids(&p).is_none() {
            return Err("会话路径超出本机历史目录或文件名无法识别".into());
        }
        Ok(p)
    }
    fn thread_location(&self, c: &Connection, id: &str) -> Result<(String, bool)> {
        Uuid::parse_str(id).map_err(err)?;
        c.query_row(
            "SELECT rollout_path, archived FROM threads WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(err)
    }
    fn archived_pointer(&self, c: &Connection, id: &str) -> Result<String> {
        let (pointer, archived) = self.thread_location(c, id)?;
        if !archived {
            return Err("请先在 ChatGPT 中归档此会话".into());
        }
        Ok(pointer)
    }
    fn resolve_rollout(&self, id: &str, pointer: &str) -> Result<Option<PathBuf>> {
        let Some(found) = rollout_file(Path::new(pointer)).map_err(err)? else {
            return Ok(None);
        };
        let actual = self.checked_path(&found)?;
        if !file_ids(&actual).is_some_and(|(owner, _)| owner == id) {
            return Err("历史文件与会话 ID 不符".into());
        }
        Ok(Some(actual))
    }
    fn current(&self, c: &Connection, id: &str) -> Result<(String, PathBuf)> {
        let pointer = self.archived_pointer(c, id)?;
        let actual = self
            .resolve_rollout(id, &pointer)?
            .ok_or("会话历史文件不存在，请刷新列表")?;
        Ok((pointer, actual))
    }
    fn is_orphaned(&self, id: &str, pointer: &str) -> Result<bool> {
        let path = PathBuf::from(plain_path(Path::new(pointer)));
        let managed = ["sessions", "archived_sessions"].iter().any(|name| {
            let root = PathBuf::from(plain_path(&self.home.join(name)));
            path.strip_prefix(root)
                .is_ok_and(|tail| tail.components().all(|c| matches!(c, Component::Normal(_))))
        });
        if !self.home.is_dir() || !managed || !file_ids(&path).is_some_and(|(owner, _)| owner == id)
        {
            return Ok(false);
        }
        Ok(rollout_file(&path).map_err(err)?.is_none()
            && !self
                .files()?
                .iter()
                .any(|file| file_ids(file).is_some_and(|(owner, _)| owner == id)))
    }
    fn prune_missing(&self, id: &str, pointer: &str) -> Result<()> {
        let _lock = self.writer(id)?;
        let mut db = self.db(true)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let unchanged = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM threads WHERE id=? AND rollout_path=? AND archived=1)",
                params![id, pointer],
                |r| r.get::<_, bool>(0),
            )
            .map_err(err)?;
        if unchanged && self.is_orphaned(id, pointer)? {
            self.delete_with_journal(tx, id, pointer, &[])?;
        }
        Ok(())
    }
    fn files(&self) -> Result<Vec<PathBuf>> {
        let mut result = Vec::new();
        let mut dirs = Vec::new();
        for name in ["sessions", "archived_sessions"] {
            let root = self.home.join(name);
            match fs::metadata(&root) {
                Ok(m) if m.is_dir() => dirs.push(root),
                Ok(_) => return Err("本机历史目录不是文件夹".into()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(err(e)),
            }
        }
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(dir).map_err(err)? {
                let e = entry.map_err(err)?;
                let t = e.file_type().map_err(err)?;
                if t.is_dir() && !t.is_symlink() {
                    dirs.push(e.path());
                } else if t.is_file() && file_ids(&e.path()).is_some() {
                    result.push(e.path());
                }
            }
        }
        Ok(result)
    }
    fn load(&self, path: &Path, id: &str) -> Result<(Vec<Value>, String)> {
        let mut leaf = read_rows(path, None)?;
        let meta = leaf.first().ok_or("会话为空")?.clone();
        if meta["type"] != "session_meta" || meta["payload"]["id"] != id {
            return Err("会话元数据不匹配".into());
        }
        // A fork can reference a frozen prefix of an ancestor by decoded byte offset.
        let mut base = meta
            .pointer("/payload/history_base")
            .cloned()
            .unwrap_or(Value::Null);
        let mut segments = vec![leaf.split_off(1)];
        let mut visited = HashSet::new();
        if !base.is_null() {
            let mut paths = HashMap::new();
            for p in self.files()? {
                if let Some((_, rid)) = file_ids(&p) {
                    paths.entry(rid).or_insert(p);
                }
            }
            while !base.is_null() {
                let rid = base["thread_id"].as_str().ok_or("分支历史引用缺少 ID")?;
                if !visited.insert(rid.to_owned()) {
                    return Err("分支历史引用形成循环".into());
                }
                let p = paths.get(rid).ok_or("分支引用的历史文件不存在")?;
                let limit = base["end_byte_offset"].as_u64().ok_or("分支缺少历史边界")?;
                let mut rows = read_rows(&self.checked_path(p)?, Some(limit))?;
                let head = rows.first().ok_or("分支父历史为空")?;
                base = head
                    .pointer("/payload/history_base")
                    .cloned()
                    .unwrap_or(Value::Null);
                segments.push(rows.split_off(1));
            }
        }
        let mut rows = vec![meta];
        for segment in segments.into_iter().rev() {
            rows.extend(segment);
        }
        let mut hash = Sha256::new();
        hash.update(path.to_string_lossy().as_bytes());
        for r in &rows {
            serde_json::to_writer(&mut hash, r).map_err(err)?;
            hash.update(b"\n");
        }
        Ok((rows, format!("{:x}", hash.finalize())))
    }
    pub fn open(&self, id: &str) -> Result<Document> {
        let (_, p) = self.current(&self.db(false)?, id)?;
        let (rows, revision) = self.load(&p, id)?;
        Ok(Document {
            revision,
            entries: items::entries(&rows),
            notice: None,
        })
    }
    pub fn unarchive(&self, id: &str, revision: &str) -> Result<()> {
        let (pointer, archived) = self.thread_location(&self.db(false)?, id)?;
        if !archived {
            return self.verify_active(id, &pointer);
        }
        let path = self
            .resolve_rollout(id, &pointer)?
            .ok_or("会话历史文件不存在，请刷新列表")?;
        if self.load(&path, id)?.1 != revision {
            return Err("会话已变化，请重新打开".into());
        }

        // Let Codex coordinate the move and its database update using its own locks.
        let output = unarchive::run(self, id)?;
        let (pointer, archived) = self.thread_location(&self.db(false)?, id)?;
        let restored = if archived {
            Err("Codex 尚未更新会话的归档状态".into())
        } else {
            self.verify_active(id, &pointer)
        };
        if restored.is_ok() || output.status.success() {
            return restored;
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().find(|line| !line.trim().is_empty());
        Err(format!(
            "取消归档失败：{}",
            detail.unwrap_or("Codex 未返回错误详情")
        ))
    }
    fn verify_active(&self, id: &str, pointer: &str) -> Result<()> {
        let path = self
            .resolve_rollout(id, pointer)?
            .ok_or("取消归档后找不到会话历史，请在 ChatGPT 中查看")?;
        let active = fs::canonicalize(self.home.join("sessions")).map_err(err)?;
        if !path.starts_with(active) {
            return Err("会话历史尚未回到普通会话目录，请在 ChatGPT 中查看".into());
        }
        Ok(())
    }
    fn writer(&self, id: &str) -> Result<(File, File)> {
        Uuid::parse_str(id).map_err(err)?;
        let dir = self.home.join("thread-writer-locks");
        fs::create_dir_all(&dir).map_err(err)?;
        let open = |p: PathBuf| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(p)
                .map_err(err)
        };
        let gate = open(dir.join(".coordination.lock"))?;
        FileExt::try_lock_exclusive(&gate)
            .map_err(|_| "客户端正在切换会话状态，请稍后重试".to_owned())?;
        let writer = open(dir.join(format!("{id}.lock")))?;
        FileExt::try_lock_exclusive(&writer)
            .map_err(|_| "此会话仍被 ChatGPT 占用，请等待会话关闭后重试。".to_owned())?;
        Ok((gate, writer))
    }
    pub fn save(&self, id: &str, expected: &str, changes: &[Change]) -> Result<Document> {
        let _lock = self.writer(id)?;
        let mut db = self.db(true)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let (pointer, path) = self.current(&tx, id)?;
        let (rows, revision) = self.load(&path, id)?;
        if expected != revision {
            return Err("会话已在别处变化，请重新打开后再编辑".into());
        }
        let mut edited = items::apply(&rows, changes)?;
        let leaf = &mut edited[0]["payload"];
        if let Some(obj) = leaf.as_object_mut() {
            obj.remove("history_base");
        }
        // Resolve boundaries before renumbering a flattened fork.
        for k in [
            "forked_from_ordinal_exclusive",
            "subagent_history_start_ordinal",
        ] {
            if let Some(old) = edited[0]["payload"][k].as_u64() {
                let boundary = edited
                    .iter()
                    .skip(1)
                    .take_while(|r| r["ordinal"].as_u64().is_some_and(|x| x < old))
                    .count()
                    + 1;
                edited[0]["payload"][k] = boundary.into();
            }
        }
        let paginated = edited[0]["payload"]["history_mode"] == "paginated";
        if paginated {
            for (i, r) in edited.iter_mut().enumerate() {
                r["ordinal"] = (i as u64).into();
            }
        }
        let file_name = format!(
            "rollout-{}-{}_{}.jsonl",
            Local::now().format("%Y-%m-%dT%H-%M-%S"),
            id,
            Uuid::new_v4()
        );
        let parent = path.parent().ok_or("会话缺少目录")?;
        let new_path = parent.join(file_name);
        let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
        {
            let mut out = BufWriter::new(staged.as_file_mut());
            for r in &edited {
                serde_json::to_writer(&mut out, r).map_err(err)?;
                out.write_all(b"\n").map_err(err)?;
            }
            out.flush().map_err(err)?;
        }
        staged.as_file().sync_all().map_err(err)?;
        let published = staged.into_temp_path();
        published.persist_noclobber(&new_path).map_err(err)?;
        let display = plain_path(&new_path);
        let messages = items::entries(&edited);
        let first = messages
            .iter()
            .find(|e| e.kind == "user")
            .map(|e| e.text.as_str())
            .unwrap_or("");
        let last = messages
            .iter()
            .rev()
            .find(|e| matches!(e.kind.as_str(), "user" | "assistant"))
            .map(|e| e.text.as_str())
            .unwrap_or("");
        let result = (|| -> Result<()> {
            let n=tx.execute("UPDATE threads SET rollout_path=?1,first_user_message=?2,preview=?3 WHERE id=?4 AND rollout_path=?5",
                params![display,first.chars().take(500).collect::<String>(),last.chars().take(500).collect::<String>(),id,pointer]).map_err(err)?;
            if n != 1 {
                return Err("会话版本已变化".into());
            }
            tx.commit().map_err(err)
        })();
        if let Err(e) = result {
            // Only our unpublished version is eligible for cleanup here.
            self.move_to_trash(&[new_path])?;
            return Err(e);
        }
        // A new rollout id gives ChatGPT a fresh pagination projection on its first read.
        // Keep immutable predecessors when a fork still addresses their offsets.
        let cleanup = (|| -> Result<()> {
            let files = self.files()?;
            if !referenced(&files, &path)? {
                let candidates = physical_siblings(&path);
                self.move_to_trash(&candidates)?;
                self.clear_projection(file_ids(&path).map(|(_, revision)| revision))?;
            }
            Ok(())
        })();
        let mut doc = self.open(id)?;
        if let Err(e) = cleanup {
            doc.notice = Some(format!("会话已保存，旧版本清理未完成：{e}"));
        }
        Ok(doc)
    }
    fn move_to_trash(&self, files: &[PathBuf]) -> Result<Vec<(PathBuf, PathBuf)>> {
        let target = self.trash.join(format!(
            "chatgpt对话助手-{}-{}",
            Local::now().format("%Y%m%d-%H%M%S"),
            Uuid::new_v4()
        ));
        fs::create_dir_all(&target).map_err(err)?;
        let mut moved: Vec<(PathBuf, PathBuf)> = vec![];
        for path in files {
            let src = self.checked_path(path)?;
            let dest = target.join(src.file_name().ok_or("无效的历史文件名")?);
            if dest.exists() {
                restore(&moved)?;
                return Err("回收目录存在同名文件".into());
            }
            if let Err(e) = fs::rename(&src, &dest) {
                restore(&moved)?;
                return Err(format!("移入回收站失败：{e}"));
            }
            moved.push((src, dest));
        }
        Ok(moved)
    }
    pub fn delete(&self, id: &str, revision: &str) -> Result<()> {
        let _lock = self.writer(id)?;
        let mut db = self.db(true)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let pointer = self.archived_pointer(&tx, id)?;
        let Some(path) = self.resolve_rollout(id, &pointer)? else {
            if self.is_orphaned(id, &pointer)? {
                return self.delete_with_journal(tx, id, &pointer, &[]);
            }
            return Err("找不到当前会话文件，请刷新列表".into());
        };
        if self.load(&path, id)?.1 != revision {
            return Err("会话已变化，请重新打开".into());
        }
        let all = self.files()?;
        let own: Vec<_> = all
            .iter()
            .filter(|p| file_ids(p).is_some_and(|(owner, _)| owner == id))
            .cloned()
            .collect();
        let others: Vec<_> = all.into_iter().filter(|p| !own.contains(p)).collect();
        for p in &own {
            if referenced(&others, p)? {
                return Err("其他会话仍引用这段历史。请先处理依赖它的分支。".into());
            }
        }
        self.delete_with_journal(tx, id, &pointer, &own)
    }
    fn delete_with_journal(
        &self,
        tx: Transaction<'_>,
        id: &str,
        pointer: &str,
        files: &[PathBuf],
    ) -> Result<()> {
        let batch = self.prepare_deletion(id, pointer, files)?;
        let moved = self.execute_moves(&batch.1)?;
        let result = (|| -> Result<()> {
            let count = tx
                .execute(
                    "DELETE FROM threads WHERE id=? AND rollout_path=? AND archived=1",
                    params![id, pointer],
                )
                .map_err(err)?;
            if count != 1 {
                return Err("删除时会话发生了变化".into());
            }
            if tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='thread_read_receipts')",
                    [],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(err)?
            {
                tx.execute("DELETE FROM thread_read_receipts WHERE thread_id=?", [id])
                    .map_err(err)?;
            }
            tx.commit().map_err(err)
        })();
        if let Err(e) = result {
            restore(&moved)?;
            return Err(e);
        }
        self.finish_deletion(&batch.0, &batch.1)
            .map_err(|e| format!("已移除会话，剩余索引将在刷新时重试清理：{e}"))
    }
}
fn restore(moved: &[(PathBuf, PathBuf)]) -> Result<()> {
    for (src, dst) in moved.iter().rev() {
        fs::rename(dst, src).map_err(err)?;
    }
    Ok(())
}
struct Project {
    id: String,
    name: String,
    root: PathBuf,
}

fn projects(db: &Connection) -> Result<Option<Vec<Project>>> {
    let tables: u32 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('projects','project_roots')", [], |r| r.get(0)).map_err(err)?;
    if tables != 2 {
        return Ok(None);
    }
    let mut q = db.prepare("SELECT p.id,p.name,coalesce(r.path,'') FROM projects p LEFT JOIN project_roots r ON r.project_id=p.id ORDER BY length(r.path) DESC").map_err(err)?;
    let rows = q
        .query_map([], |r| {
            let path: String = r.get(2)?;
            Ok(Project {
                id: r.get(0)?,
                name: r.get(1)?,
                root: PathBuf::from(plain_path(Path::new(&path)).to_lowercase()),
            })
        })
        .map_err(err)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map(Some)
        .map_err(err)
}

fn project_for(id: Option<&str>, cwd: &str, projects: &Option<Vec<Project>>) -> (String, String) {
    let dir = Path::new(cwd);
    if let Some(projects) = projects {
        let lower = PathBuf::from(cwd.to_lowercase());
        let matched = projects
            .iter()
            .find(|p| Some(p.id.as_str()) == id)
            .or_else(|| {
                projects
                    .iter()
                    .find(|p| !p.root.as_os_str().is_empty() && lower.starts_with(&p.root))
            });
        return matched
            .map(|p| (p.id.clone(), p.name.clone()))
            .unwrap_or_default();
    }
    // Older clients have no project registry. Recognize their generated chat directories.
    static GENERATED: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)[\\/]documents[\\/]codex[\\/]\d{4}-\d{2}-\d{2}[\\/][^\\/]+[\\/]?$")
            .unwrap()
    });
    if cwd.is_empty() || GENERATED.is_match(cwd) {
        return Default::default();
    }
    (
        cwd.to_lowercase(),
        dir.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| cwd.to_owned()),
    )
}

fn plain_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(x) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{x}")
    } else {
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_owned()
    }
}
fn file_ids(path: &Path) -> Option<(String, String)> {
    let name = path.file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".zst")
        .unwrap_or(name)
        .strip_suffix(".jsonl")?
        .strip_prefix("rollout-")?;
    let tail = stem.get(20..)?;
    let (owner, rev) = tail.split_once('_').unwrap_or((tail, tail));
    Some((
        Uuid::parse_str(owner).ok()?.to_string(),
        Uuid::parse_str(rev).ok()?.to_string(),
    ))
}
fn reader(p: &Path) -> Result<Box<dyn Read>> {
    let f = File::open(p).map_err(err)?;
    if p.extension().is_some_and(|s| s == "zst") {
        Ok(Box::new(zstd::Decoder::new(f).map_err(err)?))
    } else {
        Ok(Box::new(f))
    }
}
fn read_rows(p: &Path, limit: Option<u64>) -> Result<Vec<Value>> {
    let r = BufReader::new(reader(p)?.take(limit.unwrap_or(u64::MAX)));
    let mut rows = vec![];
    for line in r.lines() {
        let line = line.map_err(err)?;
        if line.trim().is_empty() {
            continue;
        }
        rows.push(
            serde_json::from_str(&line)
                .map_err(|_| "历史文件含有不完整的记录，请刷新".to_owned())?,
        );
    }
    Ok(rows)
}
fn referenced(all: &[PathBuf], candidate: &Path) -> Result<bool> {
    let (_, id) = file_ids(candidate).ok_or("无效的历史文件名")?;
    for p in all {
        let mut line = String::new();
        BufReader::new(reader(p)?)
            .read_line(&mut line)
            .map_err(err)?;
        let m: Value = serde_json::from_str(&line).map_err(err)?;
        if m.pointer("/payload/history_base/thread_id")
            .and_then(Value::as_str)
            == Some(&id)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
fn alternate_rollout(p: &Path) -> PathBuf {
    if p.extension().is_some_and(|s| s == "zst") {
        p.with_extension("")
    } else {
        let mut path = p.as_os_str().to_owned();
        path.push(".zst");
        PathBuf::from(path)
    }
}
fn rollout_file(path: &Path) -> io::Result<Option<PathBuf>> {
    for candidate in [path.to_owned(), alternate_rollout(path)] {
        match fs::symlink_metadata(&candidate) {
            Ok(m) if m.is_file() || m.is_symlink() => return Ok(Some(candidate)),
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "会话历史路径不是文件",
                ))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}
fn physical_siblings(p: &Path) -> Vec<PathBuf> {
    let peer = alternate_rollout(p);
    let mut paths = vec![p.to_owned()];
    if peer.is_file() {
        paths.push(peer);
    }
    paths
}
