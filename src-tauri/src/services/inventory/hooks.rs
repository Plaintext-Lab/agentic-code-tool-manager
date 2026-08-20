use super::config::apply_path_metadata;
use super::models::{
    ClientKind, InventoryItemType, InventoryRecord, InventoryScope, InventorySnapshot,
    InventoryWarning, SourceKind, TrustState,
};
use serde_json::{Map, Value};
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub fn push_json_hooks(
    config: &Value,
    config_path: &Path,
    client: ClientKind,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    enabled: bool,
    trust_state: TrustState,
    snapshot: &mut InventorySnapshot,
) {
    let Some(events) = config.get("hooks").and_then(Value::as_object) else {
        return;
    };
    let mut ordinal = 0;
    for (event, groups) in events {
        let Some(groups) = groups.as_array() else {
            snapshot.warnings.push(InventoryWarning::new(
                client,
                config_path.display().to_string(),
                "Skipped a hook event because its definition is not an array.",
            ));
            continue;
        };
        for group in groups {
            let Some(group_object) = group.as_object() else {
                continue;
            };
            if let Some(handlers) = group_object.get("hooks").and_then(Value::as_array) {
                for handler in handlers {
                    push_hook_record(
                        event,
                        handler.as_object(),
                        config_path,
                        client,
                        scope,
                        source_kind,
                        project_path,
                        source_priority,
                        enabled,
                        trust_state,
                        ordinal,
                        snapshot,
                    );
                    ordinal += 1;
                }
            } else {
                push_hook_record(
                    event,
                    Some(group_object),
                    config_path,
                    client,
                    scope,
                    source_kind,
                    project_path,
                    source_priority,
                    enabled,
                    trust_state,
                    ordinal,
                    snapshot,
                );
                ordinal += 1;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_hook_record(
    event: &str,
    handler: Option<&Map<String, Value>>,
    config_path: &Path,
    client: ClientKind,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    enabled: bool,
    trust_state: TrustState,
    ordinal: usize,
    snapshot: &mut InventorySnapshot,
) {
    let handler_type = handler
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "command" | "prompt" | "agent" | "http" | "mcp_tool"))
        .unwrap_or("command");
    let mut record = InventoryRecord::new(
        client,
        InventoryItemType::Hook,
        format!("{event} hook"),
        scope,
        source_kind,
        config_path.display().to_string(),
        ordinal,
        source_priority,
    );
    apply_path_metadata(&mut record, config_path);
    record.project_path = project_path.map(|path| path.display().to_string());
    record.enabled = Some(enabled);
    record.is_effective = Some(enabled);
    record.trust_state = trust_state;
    record.detail = Some(format!("{handler_type} handler"));
    snapshot.records.push(record);
}

#[allow(clippy::too_many_arguments)]
pub fn push_toml_hooks(
    config: &toml::Value,
    config_path: &Path,
    client: ClientKind,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    trust_state: TrustState,
    snapshot: &mut InventorySnapshot,
) {
    let Some(hooks) = config.get("hooks") else {
        return;
    };
    let Ok(hooks_json) = serde_json::to_value(hooks) else {
        snapshot.warnings.push(InventoryWarning::new(
            client,
            config_path.display().to_string(),
            "Could not inspect inline hook definitions.",
        ));
        return;
    };
    let enabled = config
        .get("features")
        .and_then(toml::Value::as_table)
        .and_then(|features| features.get("hooks"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
    let wrapper = serde_json::json!({ "hooks": hooks_json });
    push_json_hooks(
        &wrapper,
        config_path,
        client,
        scope,
        source_kind,
        project_path,
        source_priority,
        enabled,
        trust_state,
        snapshot,
    );
}
