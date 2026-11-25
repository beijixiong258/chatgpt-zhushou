mod opening;
mod prompts;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub key: String,
    pub turn: String,
    pub kind: String,
    pub label: String,
    pub text: String,
    pub editable: bool,
    pub removable: bool,
    pub call_group: Option<String>,
    pub default_cleanup: bool,
}

#[derive(Deserialize)]
pub struct Change {
    pub key: String,
    // null removes the selected part
    pub text: Option<String>,
}

struct Part {
    start: usize,
    end: usize,
    label: String,
}
struct Location {
    entry: Entry,
    row: usize,
    pointer: String,
    start: usize,
    end: usize,
    call: Option<String>,
}
static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"</?([A-Za-z_][\w.:-]*)(?:\s[^<>]*?)?\s*/?>").unwrap());

fn sections(s: &str, fallback: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut cursor = 0;
    for cap in TAG.captures_iter(s) {
        let m = cap.get(0).unwrap();
        let tag = cap.get(1).unwrap().as_str();
        if m.as_str().starts_with("</") {
            if stack.last() != Some(&tag) {
                continue;
            }
            stack.pop();
            if !stack.is_empty() {
                continue;
            }
        } else {
            if stack.is_empty() {
                start = m.start();
            }
            if !m.as_str().ends_with("/>") {
                stack.push(tag);
                continue;
            }
            if !stack.is_empty() {
                continue;
            }
        }
        if !s[cursor..start].trim().is_empty() {
            parts.push(Part {
                start: cursor,
                end: start,
                label: fallback.into(),
            });
        }
        parts.push(Part {
            start,
            end: m.end(),
            label: tag.into(),
        });
        cursor = m.end();
    }
    if !s[cursor..].trim().is_empty() {
        parts.push(Part {
            start: cursor,
            end: s.len(),
            label: fallback.into(),
        });
    }
    parts
}
fn role_label(role: &str) -> &str {
    match role {
        "user" => "用户",
        "assistant" => "chatgpt",
        "developer" => "开发者上下文",
        _ => "系统上下文",
    }
}
fn calls(typ: &str) -> bool {
    matches!(
        typ,
        "function_call"
            | "custom_tool_call"
            | "tool_search_call"
            | "local_shell_call"
            | "web_search_call"
            | "image_generation_call"
            | "computer_call"
            | "mcp_call"
    )
}
fn result_type(typ: &str) -> bool {
    matches!(
        typ,
        "function_call_output"
            | "custom_tool_call_output"
            | "tool_search_output"
            | "computer_call_output"
    )
}
fn val(v: &Value) -> String {
    v.as_str()
        .map(str::to_string)
        .unwrap_or_else(|| v.to_string())
}
fn field<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
fn turns(rows: &[Value]) -> Vec<String> {
    let mut current = String::new();
    rows.iter()
        .map(|r| {
            let p = &r["payload"];
            if r["type"] == "turn_context"
                || (r["type"] == "event_msg" && p["type"] == "task_started")
            {
                if let Some(id) = p["turn_id"].as_str() {
                    current = id.into();
                }
            }
            p.pointer("/internal_chat_message_metadata_passthrough/turn_id")
                .and_then(Value::as_str)
                .or_else(|| p.get("turn_id").and_then(Value::as_str))
                .unwrap_or(&current)
                .to_owned()
        })
        .collect()
}
fn add_text(
    found: &mut Vec<Location>,
    row: usize,
    turn: &str,
    pointer: &str,
    key: &str,
    text: &str,
    kind: &str,
    parts: Vec<Part>,
) {
    for (n, part) in parts.into_iter().enumerate() {
        found.push(Location {
            entry: Entry {
                key: format!("{key}.{n}"),
                turn: turn.to_owned(),
                kind: kind.into(),
                label: part.label,
                text: text[part.start..part.end].into(),
                editable: true,
                removable: true,
                call_group: None,
                default_cleanup: false,
            },
            row,
            pointer: pointer.into(),
            start: part.start,
            end: part.end,
            call: None,
        });
    }
}

fn locations(rows: &[Value]) -> Vec<Location> {
    let turn_ids = turns(rows);
    let mut found = Vec::new();
    let base = prompts::base(rows);
    if let Some(b) = &base {
        if b.message.is_none() {
            add_text(
                &mut found,
                0,
                "",
                b.pointer,
                "0.base",
                b.text,
                "system",
                prompts::chapters(b.text),
            );
        }
    }
    for (i, r) in rows.iter().enumerate() {
        let p = &r["payload"];
        if r["type"] == "turn_context" {
            for (path, label) in [
                ("/payload/developer_instructions", "developer_instructions"),
                ("/payload/user_instructions", "user_instructions"),
                (
                    "/payload/collaboration_mode/settings/developer_instructions",
                    "协作模式配置 · developer_instructions",
                ),
            ] {
                if let Some(s) = r
                    .pointer(path)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    add_text(
                        &mut found,
                        i,
                        &turn_ids[i],
                        path,
                        &format!("{i}{path}"),
                        s,
                        "setup",
                        vec![Part {
                            start: 0,
                            end: s.len(),
                            label: label.into(),
                        }],
                    );
                }
            }
        }
        if r["type"] == "compacted" {
            let summary = p.get("message").and_then(Value::as_str).unwrap_or("");
            found.push(Location {
                entry: Entry {
                    key: format!("{i}.checkpoint"),
                    turn: turn_ids[i].clone(),
                    kind: "checkpoint".into(),
                    label: "上下文压缩点".into(),
                    text: if summary.trim().is_empty() {
                        "此前记录已由客户端压缩".into()
                    } else {
                        summary.into()
                    },
                    editable: false,
                    removable: false,
                    call_group: None,
                    default_cleanup: false,
                },
                row: i,
                pointer: String::new(),
                start: 0,
                end: 0,
                call: None,
            });
        }
        if r["type"] != "response_item" {
            continue;
        }
        let typ = field(p, "type");
        if typ == "message" {
            let role = field(p, "role");
            let explicit = r
                .pointer("/metadata/user_input_order")
                .and_then(Value::as_u64)
                .is_some();
            for (j, (pointer, s, source)) in prompts::chunks(p).into_iter().enumerate() {
                let mirrored = base
                    .as_ref()
                    .and_then(|b| b.message.as_ref())
                    .is_some_and(|(row, path)| *row == i && path == &pointer);
                let system = mirrored || role == "system" || source == "model.base_instructions";
                let setup = role == "developer"
                    || (role == "user"
                        && !explicit
                        && (source.starts_with("environments.")
                            || source.starts_with("permissions.")
                            || source.starts_with("user_instructions.")
                            || source == "agents_md.instructions"
                            || s.trim_start().starts_with("# AGENTS.md instructions")
                            || s.trim_start().starts_with("<environment_context>")));
                let (kind, parts) = if system {
                    ("system", prompts::chapters(s))
                } else if setup {
                    let label = prompts::block_label(s, source, role);
                    let parts = if source.is_empty() && label != "AGENTS.md" {
                        sections(s, label)
                    } else {
                        vec![Part {
                            start: 0,
                            end: s.len(),
                            label: label.into(),
                        }]
                    };
                    ("setup", parts)
                } else if role == "assistant" {
                    (
                        "assistant",
                        vec![Part {
                            start: 0,
                            end: s.len(),
                            label: role_label(role).into(),
                        }],
                    )
                } else if role == "user" {
                    ("user", sections(s, role_label(role)))
                } else {
                    continue;
                };
                add_text(
                    &mut found,
                    i,
                    &turn_ids[i],
                    &pointer,
                    &format!("{i}.{j}"),
                    s,
                    kind,
                    parts,
                );
            }
        } else if calls(typ) || result_type(typ) {
            let call = p["call_id"]
                .as_str()
                .or_else(|| p["id"].as_str())
                .map(str::to_string);
            let payload = if result_type(typ) {
                &p["output"]
            } else {
                p.get("arguments")
                    .or_else(|| p.get("input"))
                    .or_else(|| p.get("action"))
                    .unwrap_or(p)
            };
            found.push(Location {
                entry: Entry {
                    key: format!("{i}.tool"),
                    turn: turn_ids[i].clone(),
                    kind: "tool".into(),
                    label: p["name"].as_str().unwrap_or(typ).into(),
                    text: val(payload),
                    editable: false,
                    removable: true,
                    call_group: call.clone(),
                    default_cleanup: false,
                },
                row: i,
                pointer: String::new(),
                start: 0,
                end: 0,
                call,
            });
        }
    }
    found
}
pub fn entries(rows: &[Value]) -> Vec<Entry> {
    let end = rows
        .iter()
        .position(opening::ends_opening)
        .unwrap_or(rows.len());
    let checkpoint = rows.iter().rposition(|row| {
        row["type"] == "compacted"
            || (row["type"] == "response_item" && row["payload"]["type"] == "compaction")
    });
    let base_text = rows
        .first()
        .and_then(|row| row.pointer("/payload/base_instructions"))
        .and_then(|base| base.as_str().or_else(|| base.get("text")?.as_str()));
    locations(rows)
        .into_iter()
        .map(|mut loc| {
            if loc.row < end && checkpoint.is_none_or(|i| i <= loc.row) {
                loc.entry.default_cleanup = opening::is_target(&rows[loc.row], &loc, base_text);
            }
            loc.entry
        })
        .collect()
}

fn leaf_texts(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Array(items) => {
            for x in items {
                leaf_texts(x, out)
            }
        }
        Value::String(s) => out.push(s.clone()),
        Value::Object(obj) => {
            if let Some(s) = obj.get("text").and_then(Value::as_str) {
                out.push(s.into())
            }
        }
        _ => {}
    }
}
fn event_message_role(p: &Value) -> Option<&'static str> {
    match p["item"]["type"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "usermessage" | "user_message" => Some("user"),
        "agentmessage" | "agent_message" => Some("assistant"),
        _ => None,
    }
}
fn replace_event_text(item: &mut Value, old: &[String], new: &[String]) {
    if let Some(chunks) = item["content"].as_array_mut() {
        let mut n = 0;
        for c in chunks.iter_mut() {
            if c.get("text").is_some() {
                if let Some(text) = new.get(n) {
                    c["text"] = Value::String(text.clone());
                    if c.get("text_elements").is_some() {
                        c["text_elements"] = Value::Array(vec![]);
                    }
                }
                n += 1;
            }
        }
        chunks.retain(|c| c.get("text").and_then(Value::as_str) != Some(""));
    }
    if let Some(s) = item.get("text").and_then(Value::as_str) {
        if old.len() == 1 && s == old[0] {
            item["text"] = new.first().cloned().unwrap_or_default().into();
        }
    }
}

// Resolve display events by turn, role and item id. Some client releases give a
// user bubble a different id, so use the ordered content as the fallback.
pub fn apply(rows: &[Value], changes: &[Change]) -> Result<Vec<Value>, String> {
    if changes.is_empty() {
        return Err("没有待保存的改动".into());
    }
    let loc: HashMap<_, _> = locations(rows)
        .into_iter()
        .map(|x| (x.entry.key.clone(), x))
        .collect();
    let mut seen = HashSet::new();
    let mut replacements: BTreeMap<(usize, String), Vec<(usize, usize, String)>> = BTreeMap::new();
    let mut removed = HashSet::new();
    let mut calls_to_drop = HashSet::new();
    let mut touched = HashSet::new();
    let turn_ids = turns(rows);
    let mut erased_turns = HashSet::new();
    for change in changes {
        if let Some(turn) = change.key.strip_prefix("turn/") {
            if turn.is_empty()
                || change.text.is_some()
                || !loc
                    .values()
                    .any(|x| x.entry.turn == turn && x.entry.removable)
            {
                return Err("无法定位所选轮次".into());
            }
            erased_turns.insert(turn.to_owned());
            continue;
        }
        let x = loc
            .get(&change.key)
            .ok_or("选中的内容已变化，请重新打开会话")?;
        if !seen.insert(change.key.as_str())
            || !x.entry.removable
            || (change.text.is_some() && !x.entry.editable)
        {
            return Err("无效的编辑操作".into());
        }
        touched.insert(x.row);
        if x.entry.kind == "tool" {
            removed.insert(x.row);
            if let Some(k) = &x.call {
                calls_to_drop.insert(k.clone());
            }
        } else {
            replacements
                .entry((x.row, x.pointer.clone()))
                .or_default()
                .push((x.start, x.end, change.text.clone().unwrap_or_default()));
        }
    }
    let mut out = rows.to_vec();
    for (i, r) in rows.iter().enumerate() {
        if !erased_turns.contains(&turn_ids[i]) {
            continue;
        }
        let p = &r["payload"];
        let typ = field(p, "type");
        if r["type"] == "response_item" {
            let ordinary_message = typ == "message"
                && (p["role"] == "assistant"
                    || (p["role"] == "user"
                        && !loc.values().any(|x| {
                            x.row == i && (!x.entry.removable || x.entry.kind == "setup")
                        })));
            if ordinary_message || calls(typ) || result_type(typ) || typ == "reasoning" {
                removed.insert(i);
                touched.insert(i);
                if let Some(call) = p["call_id"].as_str() {
                    calls_to_drop.insert(call.into());
                }
            }
        } else if r["type"] == "event_msg"
            && (matches!(
                typ,
                "task_started"
                    | "task_complete"
                    | "turn_started"
                    | "turn_complete"
                    | "user_message"
                    | "agent_message"
                    | "agent_reasoning"
                    | "agent_reasoning_raw_content"
                    | "item_started"
                    | "item_completed"
            ) || typ.starts_with("exec_command_")
                || typ.starts_with("mcp_tool_call_")
                || typ.starts_with("patch_apply_"))
        {
            removed.insert(i);
        }
    }
    for ((row, ptr), mut edits) in replacements {
        let value = out[row].pointer_mut(&ptr).ok_or("无法定位原文")?;
        let mut s = value.as_str().ok_or("文本类型已变化")?.to_owned();
        edits.sort_by_key(|t| std::cmp::Reverse(t.0));
        for (a, b, new) in edits {
            s.replace_range(a..b, &new);
        }
        *value = s.into();
    }
    if let Some(base) = prompts::base(rows) {
        if let Some((row, path)) = base.message {
            if touched.contains(&row) {
                let updated = out[row].pointer(&path).ok_or("无法定位开场提示词")?.clone();
                *out[0]
                    .pointer_mut(base.pointer)
                    .ok_or("无法定位开场提示词副本")? = updated;
            }
        }
    }
    // Display-only event mirrors and model-visible messages are stored separately.
    let mut mirrors: HashMap<(String, String, Vec<String>), Vec<usize>> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        let p = &r["payload"];
        if r["type"] == "response_item" && p["type"] == "message" {
            let mut signature = vec![];
            leaf_texts(&p["content"], &mut signature);
            mirrors
                .entry((turn_ids[i].clone(), field(p, "role").to_owned(), signature))
                .or_default()
                .push(i);
        }
    }
    let mut occurrence = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        let p = &r["payload"];
        let kind = field(p, "type");
        if r["type"] == "response_item"
            && (calls(kind) || result_type(kind))
            && p.get("call_id")
                .or_else(|| p.get("id"))
                .and_then(Value::as_str)
                .is_some_and(|x| calls_to_drop.contains(x))
        {
            removed.insert(i);
            touched.insert(i);
        }
        if r["type"] != "event_msg" {
            continue;
        }
        if [
            p.get("call_id"),
            p.pointer("/item/call_id"),
            p.pointer("/item/id"),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|x| calls_to_drop.contains(x))
        {
            removed.insert(i);
            continue;
        }
        let (role, content) = if let Some(role) = event_message_role(p) {
            (role, &p["item"]["content"])
        } else if kind == "user_message" {
            ("user", &p["message"])
        } else if kind == "agent_message" {
            ("assistant", &p["message"])
        } else if kind == "task_complete" {
            ("assistant", &p["last_agent_message"])
        } else {
            continue;
        };
        let mut signature = vec![];
        leaf_texts(content, &mut signature);
        let key = (turn_ids[i].clone(), role.to_string(), signature.clone());
        let Some(indices) = mirrors.get(&key) else {
            continue;
        };
        let occ = occurrence
            .entry((key.clone(), kind.to_owned()))
            .or_insert(0usize);
        let idx = indices
            .iter()
            .copied()
            .find(|j| p["item"]["id"].is_string() && p["item"]["id"] == rows[*j]["payload"]["id"])
            .or_else(|| {
                if kind == "task_complete" {
                    indices.last().copied()
                } else {
                    indices.get(*occ).copied()
                }
            });
        *occ += 1;
        let Some(idx) = idx.filter(|n| touched.contains(n)) else {
            continue;
        };
        let mut new = vec![];
        leaf_texts(&out[idx]["payload"]["content"], &mut new);
        let media = has_media(&out[idx]["payload"]["content"]);
        let p = &mut out[i]["payload"];
        if kind == "task_complete" {
            p["last_agent_message"] = if new.iter().all(|x| x.trim().is_empty()) {
                Value::Null
            } else {
                new.join("\n").into()
            };
        } else if new.iter().all(|x| x.trim().is_empty()) && !media {
            removed.insert(i);
        } else if p.get("item").is_some() {
            replace_event_text(&mut p["item"], &signature, &new);
        } else {
            p["message"] = new.join("\n").into();
        }
    }
    for i in &touched {
        let p = &mut out[*i]["payload"];
        if p["type"] == "message" {
            if let Some(chunks) = p["content"].as_array() {
                let keep: Vec<bool> = chunks
                    .iter()
                    .enumerate()
                    .map(|(n, c)| {
                        c["text"] == rows[*i]["payload"]["content"][n]["text"]
                            || !c["text"].as_str().is_some_and(|s| s.trim().is_empty())
                    })
                    .collect();
                for pointer in [
                    "/content",
                    "/internal_chat_message_metadata_passthrough/content_item_kinds",
                    "/internal_chat_message_metadata_passthrough/content_item_metadata",
                ] {
                    if let Some(xs) = p.pointer_mut(pointer).and_then(Value::as_array_mut) {
                        if xs.len() == keep.len() {
                            let mut index = 0;
                            xs.retain(|_| {
                                let k = keep[index];
                                index += 1;
                                k
                            });
                        }
                    }
                }
                if keep.iter().all(|b| !b) {
                    removed.insert(*i);
                }
            } else if p["content"].as_str().is_some_and(|s| s.trim().is_empty()) {
                removed.insert(*i);
            }
        }
    }
    let first = *touched.iter().min().ok_or("没有待编辑的记录")?;
    if rows.iter().skip(first + 1).any(|r| {
        r["type"] == "compacted"
            || (r["type"] == "response_item" && r["payload"]["type"] == "compaction")
    }) {
        return Err(
            "这次编辑跨过上下文压缩点。压缩快照可能仍包含旧内容，请编辑最近一次压缩之后的记录。"
                .into(),
        );
    }
    Ok(out
        .into_iter()
        .enumerate()
        .filter_map(|(i, r)| (!removed.contains(&i)).then_some(r))
        .collect())
}
fn has_media(v: &Value) -> bool {
    v.as_array()
        .is_some_and(|xs| xs.iter().any(|x| x.get("text").is_none()))
}

#[cfg(test)]
mod tests;
