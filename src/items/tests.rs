use super::*;
use serde_json::json;

fn msg(role: &str, text: &str) -> Value {
    json!({"type":"response_item","payload":{"type":"message","role":role,"content":[{"type":"input_text","text":text}]}})
}
fn change(e: &Entry, new: Option<&str>) -> Change {
    Change {
        key: e.key.clone(),
        text: new.map(str::to_string),
    }
}

#[test]
fn editing_one_xml_section_preserves_unicode_and_adjacent_sources() {
    let rows = vec![msg("user", "说明\n<a>第一段</a>\n<b>第二段🚀</b>")];
    let list = entries(&rows);
    let keep = apply(
        &rows,
        &[
            change(list.iter().find(|e| e.label == "a").unwrap(), None),
            change(
                list.iter().find(|e| e.label == "b").unwrap(),
                Some("<b>保留🚀</b>"),
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        keep[0]["payload"]["content"][0]["text"],
        "说明\n\n<b>保留🚀</b>"
    );
}
#[test]
fn opening_blocks_can_be_edited_individually() {
    let mut instruction = msg(
        "developer",
        "<skills_instructions>\n## Host skills\n\n- demo\n</skills_instructions>",
    );
    instruction["payload"]["content"]
        .as_array_mut()
        .unwrap()
        .push(
            json!({"type":"input_text","text":"<collaboration_mode>sample</collaboration_mode>"}),
        );
    let rows = vec![instruction, msg("user", "hello")];
    let list = entries(&rows);
    assert_eq!(
        list.iter()
            .filter(|e| e.label == "skills_instructions")
            .count(),
        1
    );
    assert!(list.iter().any(|e| e.kind == "user" && e.text == "hello"));
    let edited = apply(
        &rows,
        &[change(
            list.iter()
                .find(|e| e.label == "collaboration_mode")
                .unwrap(),
            None,
        )],
    )
    .unwrap();
    assert_eq!(
        edited[0]["payload"]["content"],
        json!([rows[0]["payload"]["content"][0]])
    );
    assert_eq!(edited[1], rows[1]);
}
#[test]
fn deleting_tool_output_removes_call_and_its_ui_events() {
    let rows = vec![
        json!({"type":"response_item","payload":{"type":"custom_tool_call","call_id":"c","input":"a"}}),
        json!({"type":"event_msg","payload":{"type":"exec_command_end","call_id":"c"}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"c","output":"b"}}),
        msg("assistant", "keep"),
    ];
    let data = entries(&rows);
    let out = apply(&rows, &[change(&data[1], None)]).unwrap();
    assert_eq!(out, vec![rows[3].clone()]);
}
#[test]
fn a_checkpoint_cannot_silently_restore_edited_old_input() {
    let rows = vec![
        msg("user", "old"),
        json!({"type":"compacted","payload":{"message":"summary","replacement_history":[]}}),
        msg("user", "later"),
    ];
    let data = entries(&rows);
    assert!(apply(&rows, &[change(&data[0], None)]).is_err());
    assert!(apply(&rows, &[change(data.last().unwrap(), None)]).is_ok());
}
#[test]
fn whole_turn_removes_private_reasoning_tools_and_display_events() {
    let rows = vec![
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"first"}}),
        msg("user", "remove"),
        json!({"type":"response_item","payload":{"type":"reasoning","encrypted_content":"sample"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"first"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"second"}}),
        msg("user", "keep"),
    ];
    let result = apply(
        &rows,
        &[Change {
            key: "turn/first".into(),
            text: None,
        }],
    )
    .unwrap();
    assert_eq!(result, rows[4..]);
}
#[test]
fn file_attachment_and_content_provenance_stay_aligned() {
    let mut row = msg("user", "delete");
    row["payload"]["content"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"input_image","image_url":"data:image/png;base64,xxxx"}));
    row["payload"]["internal_chat_message_metadata_passthrough"] =
        json!({"content_item_kinds":["user.text","user.image"],"content_item_metadata":[1,2]});
    let rows = vec![row];
    let result = apply(&rows, &[change(&entries(&rows)[0], None)]).unwrap();
    assert_eq!(result[0]["payload"]["content"].as_array().unwrap().len(), 1);
    assert_eq!(
        result[0]["payload"]["internal_chat_message_metadata_passthrough"]["content_item_kinds"],
        json!(["user.image"])
    );
}
