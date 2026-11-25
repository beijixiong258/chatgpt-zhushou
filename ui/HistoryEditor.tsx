import { setWindowState } from "./desktop";
import logo from "../assets/128x128.png";
import homeArtwork from "../assets/white-dragon.jpg";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  deleteSession,
  listSessions,
  openSession,
  saveSession,
  type Entry,
  type Session,
  type SessionDocument,
} from "./history";
import {
  Archive,
  Check,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  Clock3,
  Folder,
  House,
  ListFilter,
  RefreshCw,
  Save,
  Search,
  Trash2,
  Wrench,
  X,
} from "lucide-react";

type Draft = Record<string, string | null>;
type Decision = {
  title: string;
  detail: string;
  action: () => void;
  button: string;
};
const stamp = (n: number) =>
  new Date(n * 1000).toLocaleString("zh-CN", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
const compact = (s: string) => s.replace(/\s+/g, " ").slice(0, 110);
const isPrompt = (entry: Entry) =>
  entry.kind === "setup" || entry.kind === "system";
const promptNames: Record<string, string> = {
  Personality: "性格",
  "Writing style": "写作风格",
  "Intermediate commentary": "过程更新",
  "Final answer": "最终答复",
  "When to ask for permission": "何时请求授权",
  "When to ask the user for permission": "何时请求授权",
  "Autonomy and persistence": "自主推进与持续执行",
  "Coding Style": "编码风格",
  "Understanding the task": "理解任务",
  "Working with the user": "与用户协作",
  "Rules for getting work done": "工作规则",
  "Using skills": "使用技能",
  "When to use a skill": "何时使用技能",
  "How to use skills": "如何使用技能",
  "How to use plugins": "如何使用插件",
  "Plugins and Apps": "插件与应用",
  Plugins: "插件",
  skills_instructions: "主机技能",
  collaboration_mode: "协作模式",
  multi_agent_role: "多智能体角色",
  multi_agent_mode: "多智能体模式",
  "app-context": "桌面端上下文",
};
const category: Record<string, string> = {
  user: "用户",
  assistant: "chatgpt",
  tool: "工具",
  system: "系统提示词",
  setup: "开场",
  checkpoint: "压缩",
};

export function HistoryEditor() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [id, setId] = useState("");
  const [doc, setDoc] = useState<SessionDocument | null>(null);
  const [draft, setDraft] = useState<Draft>({});
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState("全部");
  const [group, setGroup] = useState("项目");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [note, setNote] = useState("");
  const [decision, setDecision] = useState<Decision | null>(null);
  const [batchMode, setBatchMode] = useState(false);
  const [batch, setBatch] = useState<Set<string>>(new Set());
  const generation = useRef(0);
  const pane = useRef<HTMLElement>(null);
  const dirty = Object.keys(draft).length;
  const current = sessions.find((s) => s.id === id);
  const refresh = async () => {
    const data = await listSessions();
    setSessions(data);
  };
  useEffect(() => {
    refresh().catch((e) => setError(String(e)));
  }, []);
  useEffect(() => {
    setWindowState(Boolean(dirty), busy);
    const handle = (ev: BeforeUnloadEvent) => {
      if (dirty) {
        ev.preventDefault();
        ev.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", handle);
    return () => window.removeEventListener("beforeunload", handle);
  }, [dirty, busy]);
  function guard(action: () => void) {
    if (dirty)
      setDecision({
        title: "放弃未保存的编辑？",
        detail: "当前会话的草稿将被丢弃。",
        button: "放弃草稿",
        action,
      });
    else action();
  }
  function returnHome() {
    ++generation.current;
    setId("");
    setDoc(null);
    setDraft({});
    setLoading(false);
    setError("");
    setNote("");
  }
  async function read(sessionId: string) {
    const token = ++generation.current;
    setId(sessionId);
    setDoc(null);
    setDraft({});
    setExpanded(new Set());
    setError("");
    setNote("");
    setLoading(true);
    try {
      const data = await openSession(sessionId);
      if (token === generation.current) {
        setDoc(data);
        pane.current?.scrollTo(0, 0);
      }
    } catch (e) {
      if (token === generation.current) setError(String(e));
    } finally {
      if (token === generation.current) setLoading(false);
    }
  }
  const filtered = useMemo(() => {
    const term = query.toLocaleLowerCase().trim();
    return sessions.filter(
      (s) =>
        (scope === "全部" || (scope === "已归档") === s.archived) &&
        [s.title, s.preview, s.project_name, s.cwd, s.id].some((v) =>
          v.toLocaleLowerCase().includes(term),
        ),
    );
  }, [sessions, query, scope]);
  const groups = useMemo(() => {
    const m = new Map<string, Session[]>();
    for (const s of filtered) {
      const k =
        group === "项目"
          ? s.project
          : new Date(s.updated * 1000).toLocaleDateString("zh-CN");
      const bucket = m.get(k);
      if (bucket) bucket.push(s);
      else m.set(k, [s]);
    }
    const unassigned = group === "项目" && m.get("");
    if (unassigned) {
      m.delete("");
      m.set("", unassigned);
    }
    return [...m];
  }, [filtered, group]);
  const entries = doc?.entries ?? [];
  const cleanupItems = entries.filter(
    (entry) => entry.defaultCleanup && draft[entry.key] !== null,
  );
  const timeline: { key: string; entries: Entry[] }[] = [];
  for (const entry of entries) {
    const key =
      entry.kind === "system"
        ? entry.key.slice(0, entry.key.lastIndexOf("."))
        : entry.key;
    const last = timeline[timeline.length - 1];
    if (entry.kind === "system" && last?.key === key) last.entries.push(entry);
    else timeline.push({ key, entries: [entry] });
  }
  function toggle(set: Set<string>, k: string) {
    const next = new Set(set);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    return next;
  }
  function edit(key: string, value: string | null, original?: string) {
    setDraft((prev) => {
      const next = { ...prev };
      if (value === original) delete next[key];
      else next[key] = value;
      return next;
    });
    setNote("");
  }
  function mark(items: Entry[]) {
    setDraft((prev) => {
      const next = { ...prev };
      const pairs = new Set(
        items.filter((x) => x.callGroup).map((x) => x.callGroup),
      );
      for (const e of items) if (e.removable) next[e.key] = null;
      for (const e of entries)
        if (e.callGroup && pairs.has(e.callGroup)) next[e.key] = null;
      return next;
    });
    setNote("");
  }
  async function save() {
    if (!doc || !dirty || busy) return;
    setBusy(true);
    setError("");
    try {
      const changes = Object.entries(draft).map(([key, text]) => ({
        key,
        text,
      }));
      const next = await saveSession(id, doc.revision, changes);
      setDoc(next);
      setDraft({});
      setExpanded(new Set());
      setNote(next.notice || "已保存");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function remove(ids: string[]) {
    setBusy(true);
    setError("");
    let count = 0;
    try {
      for (const s of ids) {
        const revision =
          s === id && doc ? doc.revision : (await openSession(s)).revision;
        await deleteSession(s, revision);
        count++;
        setBatch((prev) => {
          const n = new Set(prev);
          n.delete(s);
          return n;
        });
        if (s === id) returnHome();
      }
      setNote("已移入垃圾桶");
    } catch (e) {
      setError((count ? "已移入 " + count + " 个会话。" : "") + String(e));
    } finally {
      await refresh().catch((e) => setError(String(e)));
      setBusy(false);
    }
  }
  function confirmDelete(ids: string[]) {
    if (!ids.length) return;
    setDecision({
      title: "将" + ids.length + " 个会话移入垃圾桶？",
      detail: "会话将从 ChatGPT 的列表中移除，原文件移入桌面的智能体回收站。",
      button: "移入垃圾桶",
      action: () => void remove(ids),
    });
  }
  const pending = (key: string) =>
    Object.prototype.hasOwnProperty.call(draft, key);
  function renderEntry(e: Entry) {
    const context = isPrompt(e);
    const removed =
      (!context && e.removable && pending("turn/" + e.turn)) ||
      (pending(e.key) && draft[e.key] === null);
    const opened = expanded.has(e.key);
    const value = draft[e.key] ?? e.text;
    return (
      <article
        key={e.key}
        className={"entry " + e.kind + (removed ? " removed" : "")}
      >
        <div className="entry-head">
          <button
            className="entry-name"
            aria-expanded={opened}
            onClick={() => setExpanded((prev) => toggle(prev, e.key))}
          >
            {opened ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            <span>
              {context && promptNames[e.label]
                ? `${promptNames[e.label]} · ${e.label}`
                : e.label}
            </span>
          </button>
          <small>
            {e.label !== category[e.kind] && <>{category[e.kind]} · </>}
            {e.text.length.toLocaleString()} 字符
          </small>
          <span className="grow" />
          {pending(e.key) && (
            <button onClick={() => edit(e.key, e.text, e.text)}>撤销</button>
          )}
          {e.removable && !removed && (
            <button
              title="删除此项"
              aria-label={"删除 " + e.label}
              disabled={busy}
              onClick={() => mark([e])}
            >
              <Trash2 size={14} />
            </button>
          )}
        </div>
        {opened && !removed ? (
          e.editable ? (
            <>
              <textarea
                aria-label={"编辑 " + e.label}
                spellCheck={false}
                value={value}
                disabled={busy}
                onChange={(ev) => edit(e.key, ev.target.value, e.text)}
              />
              {["user", "setup", "system"].includes(e.kind) &&
                entries.filter(
                  (x) =>
                    x.removable && x.kind === e.kind && x.label === e.label,
                ).length > 1 &&
                !["用户", "chatgpt"].includes(e.label) && (
                  <button
                    className="tag-action"
                    onClick={() =>
                      mark(
                        entries.filter(
                          (x) =>
                            x.kind === e.kind &&
                            x.label === e.label &&
                            x.removable,
                        ),
                      )
                    }
                  >
                    删除本会话所有同名标签
                  </button>
                )}
            </>
          ) : (
            <pre>{e.text}</pre>
          )
        ) : removed ? (
          <p>待删除</p>
        ) : (
          !context && <p>{compact(value)}</p>
        )}
      </article>
    );
  }

  return (
    <div className="shell">
      <aside className="library">
        <header className="brand">
          <span className="brandmark">
            <img src={logo} alt="" />
          </span>
          <strong>chatgpt对话助手</strong>
        </header>
        <div className="list-head">
          <h1>会话</h1>
          <span className="count">{sessions.length}</span>
          <div className="grow" />
          <button
            title="刷新列表"
            aria-label="刷新列表"
            disabled={busy}
            onClick={() => refresh().catch((e) => setError(String(e)))}
          >
            <RefreshCw size={16} />
          </button>
          <button
            title="批量选择"
            aria-label="批量选择"
            aria-pressed={batchMode}
            onClick={() => {
              setBatchMode(!batchMode);
              setBatch(new Set());
            }}
          >
            <ListFilter size={17} />
          </button>
        </div>
        <div className="search">
          <Search size={15} />
          <input
            aria-label="搜索会话"
            placeholder="搜索标题、目录或会话 ID"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <div className="filters">
          <select
            aria-label="归档状态"
            value={scope}
            onChange={(e) => setScope(e.target.value)}
          >
            {["全部", "未归档", "已归档"].map((x) => (
              <option key={x}>{x}</option>
            ))}
          </select>
          <div className="seg">
            {["项目", "时间"].map((x) => (
              <button
                key={x}
                className={group === x ? "chosen" : ""}
                onClick={() => setGroup(x)}
              >
                {x}
              </button>
            ))}
          </div>
        </div>
        {batchMode && (
          <div className="batch">
            <button
              disabled={busy}
              onClick={() => setBatch(new Set(filtered.map((s) => s.id)))}
            >
              全选结果
            </button>
            <button
              className="danger"
              disabled={busy || !batch.size}
              onClick={() => confirmDelete([...batch])}
            >
              删除所选（{batch.size}）
            </button>
          </div>
        )}
        <div className="session-list">
          {!groups.length && <div className="empty-list">没有匹配的会话</div>}
          {groups.map(([key, ss]) => (
            <div key={key}>
              <button
                className="group-head"
                title={group === "项目" ? ss[0].project_name || "无项目" : key}
                onClick={() => setCollapsed((prev) => toggle(prev, key))}
              >
                {collapsed.has(key) ? (
                  <ChevronRight size={13} />
                ) : (
                  <ChevronDown size={13} />
                )}{" "}
                {group === "项目" ? <Folder size={14} /> : <Clock3 size={14} />}
                <span>
                  {group === "项目" ? ss[0].project_name || "无项目" : key}
                </span>
                <small>{ss.length}</small>
              </button>
              {!collapsed.has(key) &&
                ss.map((s) => (
                  <button
                    key={s.id}
                    className={"session" + (id === s.id ? " active" : "")}
                    disabled={busy}
                    onClick={() =>
                      batchMode
                        ? setBatch((prev) => toggle(prev, s.id))
                        : id !== s.id && guard(() => void read(s.id))
                    }
                  >
                    {batchMode && (
                      <span className={"box " + (batch.has(s.id) ? "on" : "")}>
                        {batch.has(s.id) && <Check size={11} />}
                      </span>
                    )}
                    <div>
                      <strong>{s.title}</strong>
                      <p>{compact(s.preview) || s.id}</p>
                      <small>
                        {s.archived && (
                          <>
                            <Archive size={11} />
                            已归档 ·{" "}
                          </>
                        )}
                        {stamp(s.updated)}
                      </small>
                    </div>
                  </button>
                ))}
            </div>
          ))}
        </div>
      </aside>
      <section className="workspace">
        {current && (
          <header className="topbar">
            <button disabled={busy} onClick={() => guard(returnHome)}>
              <House size={15} aria-hidden="true" /> 首页
            </button>
            <ChevronRight size={13} aria-hidden="true" />
            <span>ChatGPT 本地历史</span>
            <span className="grow" />
            <button
              disabled={busy || loading || !doc}
              className="quiet-danger"
              onClick={() => confirmDelete([id])}
            >
              <Trash2 size={14} />
              删除会话
            </button>
          </header>
        )}
        {error && (
          <div className="error" role="alert">
            <CircleAlert size={16} />
            {error}
            <button aria-label="关闭错误消息" onClick={() => setError("")}>
              <X size={13} />
            </button>
          </div>
        )}
        {current ? (
          <>
            <div className="reader-head">
              <div className="path">{current.project_name || "无项目"}</div>
              <h2>{current.title}</h2>
              <div className="toolbar">
                <button
                  className="primary"
                  disabled={!doc || busy || loading || !cleanupItems.length}
                  onClick={() => mark(cleanupItems)}
                  title="按指定的 18 项标记当前会话的开场提示词"
                >
                  加油白龙娘
                </button>
                <span className="grow" />
                <button
                  disabled={
                    !doc || busy || !entries.some((e) => e.kind === "tool")
                  }
                  onClick={() => mark(entries.filter((e) => e.kind === "tool"))}
                >
                  <Wrench size={14} />
                  清除工具历史
                </button>
              </div>
            </div>
            <main className="timeline" ref={pane}>
              {loading ? (
                <div className="empty-list">读取中…</div>
              ) : (
                timeline.map((block, index) => {
                  const e = block.entries[0];
                  const turnStart =
                    e.turn && e.turn !== timeline[index - 1]?.entries[0].turn;
                  const erasedTurn = pending("turn/" + e.turn);
                  return (
                    <div key={block.key}>
                      {turnStart && (
                        <div className="turn">
                          <span>对话轮次</span>
                          <code>{e.turn.slice(-8)}</code>
                          <div className="grow" />
                          {entries.some(
                            (x) =>
                              x.turn === e.turn && x.removable && !isPrompt(x),
                          ) && (
                            <button
                              disabled={busy}
                              onClick={() =>
                                setDraft((prev) => {
                                  const next = { ...prev };
                                  if (erasedTurn) {
                                    delete next["turn/" + e.turn];
                                    return next;
                                  }
                                  for (const item of entries.filter(
                                    (x) => x.turn === e.turn && !isPrompt(x),
                                  ))
                                    delete next[item.key];
                                  next["turn/" + e.turn] = null;
                                  return next;
                                })
                              }
                            >
                              {erasedTurn ? "撤销整轮删除" : "删除整轮"}
                            </button>
                          )}
                        </div>
                      )}
                      {e.kind === "system" ? (
                        <details className="system-prompt">
                          <summary>
                            系统提示词{" "}
                            <span>system · {block.entries.length} 段</span>
                          </summary>
                          <button
                            className="quiet-danger"
                            disabled={
                              busy ||
                              block.entries.every(
                                (item) => draft[item.key] === null,
                              )
                            }
                            onClick={() => mark(block.entries)}
                          >
                            <Trash2 size={14} /> 删除整份系统提示词
                          </button>
                          {block.entries.map(renderEntry)}
                        </details>
                      ) : (
                        renderEntry(e)
                      )}
                    </div>
                  );
                })
              )}
            </main>
            <footer className="savebar">
              <span role="status">
                {busy
                  ? "处理中…"
                  : dirty
                    ? dirty + " 项待保存"
                    : note || "无待保存的改动"}
              </span>
              <span className="grow" />
              {dirty > 0 && (
                <button disabled={busy} onClick={() => setDraft({})}>
                  撤销全部
                </button>
              )}
              <button
                className="primary"
                disabled={!dirty || busy}
                onClick={save}
              >
                <Save size={15} />
                保存修改
              </button>
            </footer>
          </>
        ) : (
          <main className="home-page" aria-labelledby="home-title">
            <img
              className="home-art"
              src={homeArtwork}
              width={1312}
              height={1199}
              alt="白龙娘在月光下托腮坐在书桌前，桌边卧着一只小白龙"
            />
            <div className="home-copy">
              <h2 id="home-title">白龙娘的会话书房</h2>
              <p>
                从左侧选一段 ChatGPT
                本地会话，就能编辑开场提示词、删除消息，或清理工具调用记录。
              </p>
              <p className="home-save">
                <Save size={16} aria-hidden="true" />
                改动先留在草稿里，点「保存修改」后写回。
              </p>
              {note && <p role="status">{note}</p>}
            </div>
          </main>
        )}
      </section>
      {decision && (
        <div className="overlay">
          <section className="modal" role="alertdialog" aria-modal="true">
            <h3>{decision.title}</h3>
            <p>{decision.detail}</p>
            <div>
              <button autoFocus onClick={() => setDecision(null)}>
                取消
              </button>
              <button
                className="danger"
                onClick={() => {
                  const fn = decision.action;
                  setDecision(null);
                  fn();
                }}
              >
                {decision.button}
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
