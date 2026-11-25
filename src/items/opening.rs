use super::{calls, field, prompts, result_type, Location};
use serde_json::Value;

pub(super) fn ends_opening(row: &Value) -> bool {
    let p = &row["payload"];
    let typ = field(p, "type");
    match field(row, "type") {
        "event_msg" => matches!(
            typ,
            "user_message" | "agent_message" | "task_complete" | "turn_complete"
        ),
        "compacted" => true,
        "response_item" if typ == "message" && p["role"] == "user" => {
            if row
                .pointer("/metadata/user_input_order")
                .is_some_and(|x| !x.is_null())
            {
                return true;
            }
            if p.pointer("/internal_chat_message_metadata_passthrough/content_item_kinds")
                .and_then(Value::as_array)
                .is_some_and(|kinds| {
                    kinds
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|s| s.starts_with("user."))
                })
                || p["content"]
                    .as_array()
                    .is_some_and(|xs| xs.iter().any(|x| x.get("text").is_none()))
            {
                return true;
            }
            let chunks = prompts::chunks(p);
            chunks.is_empty()
                || chunks.iter().any(|(_, text, source)| {
                    // Older logs sometimes omit provenance for injected AGENTS.md and environment text.
                    let injected = source.starts_with("agents_md.")
                        || source.starts_with("user_instructions.")
                        || source.starts_with("environments.")
                        || source.starts_with("permissions.")
                        || source.starts_with("additional_content.codex_apps_")
                        || (source.is_empty()
                            && (text.trim_start().starts_with("# AGENTS.md instructions")
                                || text.trim_start().starts_with("<environment_context>")));
                    !injected
                })
        }
        "response_item" => {
            p["role"] == "assistant"
                || typ == "reasoning"
                || typ == "compaction"
                || calls(typ)
                || result_type(typ)
        }
        _ => false,
    }
}

const SYSTEM_SECTIONS: &[&str] = &[
    "开头",
    "When to ask the user for permission",
    "Autonomy and persistence",
    "Personality",
    "Writing style",
    "Technical communication",
    "Writing PR descriptions",
    "Working with the user",
    "Intermediate commentary",
    "Final answer",
    "Formatting rules",
    "Rules for getting work done",
    "Decision Boundary",
    "Frontend Design Guidance",
    "Coding Style",
    "Visualizations",
    "Other instructions:",
];

const DEVELOPER_BLOCKS: &[(&str, &str)] = &[
    ("skills_instructions", "host_skills.instructions"),
    ("collaboration_mode", "collaboration_mode.instructions"),
    ("tools", "tools.deferred_namespaces"),
    ("recommended_plugins", "plugins.recommendations"),
    ("multi_agent_role", "multi_agent.role_instructions"),
    ("multi_agent_mode", "multi_agent.mode_instructions"),
];

pub(super) fn is_target(row: &Value, loc: &Location, base_text: Option<&str>) -> bool {
    let entry = &loc.entry;
    let label = entry.label.as_str();
    let p = &row["payload"];
    if !matches!(entry.kind.as_str(), "system" | "setup") {
        return false;
    }
    if row["type"] == "turn_context" {
        return loc.pointer == "/payload/collaboration_mode/settings/developer_instructions";
    }
    let index = loc
        .pointer
        .split('/')
        .nth(3)
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(0);
    let sources = p.pointer("/internal_chat_message_metadata_passthrough/content_item_kinds");
    let source = sources
        .and_then(|kinds| kinds.get(index))
        .and_then(Value::as_str)
        .unwrap_or("");
    if entry.kind == "system" {
        if row["type"] != "session_meta"
            && source != "model.base_instructions"
            && !base_text
                .is_some_and(|s| row.pointer(&loc.pointer).and_then(Value::as_str) == Some(s))
            && !(p["role"] == "system"
                && source.is_empty()
                && row
                    .pointer(&loc.pointer)
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.trim_start().starts_with("You are Codex")))
        {
            return false;
        }
        // Some templates shorten the permission heading but keep the same section.
        return SYSTEM_SECTIONS
            .iter()
            .any(|s| label.eq_ignore_ascii_case(s))
            || label.eq_ignore_ascii_case("When to ask for permission");
    }
    if row["type"] != "response_item" || p["type"] != "message" {
        return false;
    }
    let role = field(p, "role");
    if let Some((_, expected_source)) = DEVELOPER_BLOCKS.iter().find(|(tag, _)| *tag == label) {
        if role != "developer" {
            return false;
        }
        if source == *expected_source {
            return true;
        }
        // Without a specific source, only select a self-contained tagged block.
        let text = entry.text.trim();
        let closing = format!("</{label}>");
        return matches!(source, "" | "unknown")
            && prompts::block_label(text, "", role) == label
            && text
                .split_once(closing.as_str())
                .is_some_and(|(_, trailing)| trailing.is_empty());
    }
    // Old clients can put several unrelated blocks inside one text chunk.
    if entry.text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("# AGENTS.md instructions")
            || line.starts_with("<INSTRUCTIONS>")
            || line
                .strip_prefix('<')
                .and_then(|rest| rest.split_once('>'))
                .is_some_and(|(tag, _)| DEVELOPER_BLOCKS.iter().any(|(known, _)| *known == tag))
    }) {
        return false;
    }
    match (role, label) {
        ("developer", "app-context") => {
            matches!(source, "" | "unknown" | "generic.developer_instructions")
        }
        ("developer", "permissions") => {
            matches!(source, "" | "unknown" | "permissions.instructions")
        }
        ("developer", "codex_apps_client_time_context") => matches!(
            source,
            "" | "unknown" | "additional_content.codex_apps_client_time_context"
        ),
        // A real user can paste this same XML. Only select an attributable injected chunk.
        ("user", "environment_context") => {
            source == "environments.environment_context"
                && row
                    .pointer("/metadata/user_input_order")
                    .is_none_or(Value::is_null)
        }
        _ => false,
    }
}
