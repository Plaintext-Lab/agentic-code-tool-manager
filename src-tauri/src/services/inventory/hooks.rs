use super::config::apply_path_metadata;
use super::models::{
    effective_state, ActionBlockedReason, ClientKind, InventoryItemType, InventoryRecord,
    InventoryScope, InventorySnapshot, InventoryWarning, SourceKind, TrustState,
};
use serde_json::{Map, Value};
use std::path::Path;

mod codex_trust;

use codex_trust::{codex_effective_state, CodexHookState};

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
    push_hooks(
        config,
        config_path,
        client,
        scope,
        source_kind,
        project_path,
        source_priority,
        enabled,
        trust_state,
        None,
        snapshot,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn push_codex_json_hooks(
    config: &Value,
    config_path: &Path,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    enabled: bool,
    trust_state: TrustState,
    state_config: Option<&toml::Value>,
    key_source: &str,
    snapshot: &mut InventorySnapshot,
) {
    let codex_state = CodexHookState {
        key_source,
        config: state_config,
    };
    push_hooks(
        config,
        config_path,
        ClientKind::Codex,
        scope,
        source_kind,
        project_path,
        source_priority,
        enabled,
        trust_state,
        Some(&codex_state),
        snapshot,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_hooks(
    config: &Value,
    config_path: &Path,
    client: ClientKind,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    enabled: bool,
    trust_state: TrustState,
    codex_state: Option<&CodexHookState<'_>>,
    snapshot: &mut InventorySnapshot,
) {
    let Some(events) = config.get("hooks").and_then(Value::as_object) else {
        return;
    };
    let mut ordinal = 0;
    for (event, groups) in events {
        let Some(groups) = groups.as_array() else {
            snapshot.warnings.push(InventoryWarning::blocked(
                client,
                config_path.display().to_string(),
                "Skipped a hook event because its definition is not an array.",
                ActionBlockedReason::MalformedSource,
            ));
            continue;
        };
        for (group_index, group) in groups.iter().enumerate() {
            let Some(group_object) = group.as_object() else {
                continue;
            };
            if let Some(handlers) = group_object.get("hooks").and_then(Value::as_array) {
                for (handler_index, handler) in handlers.iter().enumerate() {
                    let Some(handler) = handler.as_object() else {
                        push_invalid_handler_warning(client, config_path, snapshot);
                        continue;
                    };
                    if push_hook_record(
                        event,
                        group_object,
                        handler,
                        group_index,
                        handler_index,
                        config_path,
                        client,
                        scope,
                        source_kind,
                        project_path,
                        source_priority,
                        enabled,
                        trust_state,
                        ordinal,
                        codex_state,
                        snapshot,
                    ) {
                        ordinal += 1;
                    }
                }
            } else if push_hook_record(
                event,
                group_object,
                group_object,
                group_index,
                0,
                config_path,
                client,
                scope,
                source_kind,
                project_path,
                source_priority,
                enabled,
                trust_state,
                ordinal,
                codex_state,
                snapshot,
            ) {
                ordinal += 1;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_hook_record(
    event: &str,
    group: &Map<String, Value>,
    handler: &Map<String, Value>,
    group_index: usize,
    handler_index: usize,
    config_path: &Path,
    client: ClientKind,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    enabled: bool,
    trust_state: TrustState,
    ordinal: usize,
    codex_state: Option<&CodexHookState<'_>>,
    snapshot: &mut InventorySnapshot,
) -> bool {
    let Some(handler_type) = valid_handler_type(handler) else {
        push_invalid_handler_warning(client, config_path, snapshot);
        return false;
    };
    let hooks_feature_enabled = enabled;
    let codex_evaluation = codex_state.map(|state| {
        codex_effective_state(
            state,
            event,
            group,
            handler,
            handler_type,
            group_index,
            handler_index,
            trust_state,
        )
    });
    let enabled = codex_evaluation
        .as_ref()
        .map_or(enabled, |evaluation| evaluation.enabled);
    let trust_state = codex_evaluation
        .as_ref()
        .map_or(trust_state, |evaluation| evaluation.trust_state);
    let approval_pending = codex_evaluation
        .as_ref()
        .is_some_and(|evaluation| evaluation.approval_pending);
    let state_key = codex_evaluation
        .as_ref()
        .map(|evaluation| evaluation.state_key.clone());
    let action_supported = codex_evaluation
        .as_ref()
        .is_none_or(|evaluation| evaluation.action_supported);
    let mut record = InventoryRecord::new(
        client,
        InventoryItemType::Hook,
        format!("{event} hook"),
        scope,
        source_kind,
        config_path.display().to_string(),
        project_path,
        ordinal,
        source_priority,
    );
    apply_path_metadata(&mut record, config_path);
    record.enabled = Some(enabled);
    record.trust_state = trust_state;
    record.is_effective = effective_state(enabled && hooks_feature_enabled, trust_state);
    record.approval_pending = approval_pending;
    record.codex_hook_state_key = state_key;
    if !action_supported {
        record.restrict_actions(ActionBlockedReason::UnsupportedByClient);
    }
    record.detail = Some(format!("{handler_type} handler"));
    snapshot.records.push(record);
    true
}

fn valid_handler_type(handler: &Map<String, Value>) -> Option<&str> {
    let handler_type = handler
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| handler.get("command").is_some().then_some("command"))?;
    let non_empty = |key: &str| {
        handler
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    };
    match handler_type {
        "command" if non_empty("command") => Some(handler_type),
        "prompt" | "agent" if non_empty("prompt") => Some(handler_type),
        "http" if non_empty("url") => Some(handler_type),
        "mcp_tool" if non_empty("server") && non_empty("tool") => Some(handler_type),
        _ => None,
    }
}

fn push_invalid_handler_warning(
    client: ClientKind,
    config_path: &Path,
    snapshot: &mut InventorySnapshot,
) {
    snapshot.warnings.push(InventoryWarning::blocked(
        client,
        config_path.display().to_string(),
        "Skipped a hook handler because it has no usable payload.",
        ActionBlockedReason::MalformedSource,
    ));
}

#[allow(clippy::too_many_arguments)]
pub fn push_toml_hooks(
    config: &toml::Value,
    config_path: &Path,
    scope: InventoryScope,
    source_kind: SourceKind,
    project_path: Option<&Path>,
    source_priority: u16,
    enabled: bool,
    trust_state: TrustState,
    state_config: Option<&toml::Value>,
    snapshot: &mut InventorySnapshot,
) {
    let Some(wrapper) = toml_hooks_wrapper(config, config_path, snapshot) else {
        return;
    };
    push_codex_json_hooks(
        &wrapper,
        config_path,
        scope,
        source_kind,
        project_path,
        source_priority,
        enabled,
        trust_state,
        state_config,
        &config_path.display().to_string(),
        snapshot,
    );
}

fn toml_hooks_wrapper(
    config: &toml::Value,
    config_path: &Path,
    snapshot: &mut InventorySnapshot,
) -> Option<Value> {
    let hooks = config.get("hooks")?;
    let Ok(mut hooks_json) = serde_json::to_value(hooks) else {
        snapshot.warnings.push(InventoryWarning::blocked(
            ClientKind::Codex,
            config_path.display().to_string(),
            "Could not inspect inline hook definitions.",
            ActionBlockedReason::MalformedSource,
        ));
        return None;
    };
    if let Some(hooks) = hooks_json.as_object_mut() {
        hooks.remove("state");
    }
    Some(serde_json::json!({ "hooks": hooks_json }))
}
