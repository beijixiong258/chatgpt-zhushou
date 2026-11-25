use super::{field, Part};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

pub(super) fn chunks(p: &Value) -> Vec<(String, &str, &str)> {
    let sources = p.pointer("/internal_chat_message_metadata_passthrough/content_item_kinds");
    let source = |i: usize| {
        sources
            .and_then(|v| v.get(i))
            .and_then(Value::as_str)
            .unwrap_or("")
    };
    match &p["content"] {
        Value::String(s) => vec![("/payload/content".into(), s, source(0))],
        Value::Array(items) => items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                Some((
                    format!("/payload/content/{i}/text"),
                    item.get("text")?.as_str()?,
                    source(i),
                ))
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) struct Base<'a> {
    pub pointer: &'static str,
    pub text: &'a str,
    pub message: Option<(usize, String)>,
}

pub(super) fn base(rows: &[Value]) -> Option<Base<'_>> {
    let header = rows.first()?;
    if header["type"] != "session_meta" {
        return None;
    }
    let (pointer, text) = [
        "/payload/base_instructions/text",
        "/payload/base_instructions",
    ]
    .into_iter()
    .find_map(|p| Some((p, header.pointer(p)?.as_str()?)))?;
    let mut fallback = None;
    for (i, row) in rows.iter().enumerate().skip(1) {
        let p = &row["payload"];
        if row["type"] != "response_item"
            || p["type"] != "message"
            || !matches!(field(p, "role"), "system" | "developer")
        {
            continue;
        }
        for (path, body, source) in chunks(p) {
            if body != text {
                continue;
            }
            if source == "model.base_instructions" {
                return Some(Base {
                    pointer,
                    text,
                    message: Some((i, path)),
                });
            }
            if source.is_empty() && fallback.is_none() {
                fallback = Some((i, path));
            }
        }
    }
    Some(Base {
        pointer,
        text,
        message: fallback,
    })
}

pub(super) fn chapters(s: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    let mut label = "开头".to_owned();
    let mut fence: Option<(char, usize)> = None;
    for line in s.split_inclusive('\n') {
        let t = line.trim();
        let c = t.chars().next().unwrap_or(' ');
        let run = t.chars().take_while(|v| *v == c).count();
        if matches!(c, '`' | '~') && run >= 3 {
            if let Some((mark, n)) = fence {
                if c == mark && run >= n && t[run..].is_empty() {
                    fence = None;
                }
            } else {
                fence = Some((c, run));
            }
        } else if fence.is_none() && c == '#' && run <= 6 && t[run..].starts_with(' ') {
            if offset > start {
                parts.push(Part {
                    start,
                    end: offset,
                    label,
                });
            }
            start = offset;
            label = t[run..].trim().to_owned();
        }
        offset += line.len();
    }
    if start < s.len() {
        parts.push(Part {
            start,
            end: s.len(),
            label,
        });
    }
    parts
}

pub(super) fn block_label<'a>(s: &'a str, source: &'a str, role: &str) -> &'a str {
    if source == "agents_md.instructions" || s.trim_start().starts_with("# AGENTS.md") {
        return "AGENTS.md";
    }
    static OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^<([\w.:-]+)(?:\s|>)").unwrap());
    if let Some(m) = OPEN.captures(s.trim_start()).and_then(|c| c.get(1)) {
        return m.as_str();
    }
    if !source.is_empty() {
        return source;
    }
    if role == "system" {
        "system"
    } else {
        "开发者上下文"
    }
}
