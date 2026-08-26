use super::super::models::TrustState;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub(super) struct CodexHookState<'a> {
    pub(super) key_source: &'a str,
    pub(super) config: Option<&'a toml::Value>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn codex_effective_state(
    state: &CodexHookState<'_>,
    event: &str,
    group: &Map<String, Value>,
    handler: &Map<String, Value>,
    handler_type: &str,
    group_index: usize,
    handler_index: usize,
    enabled: bool,
    base_trust: TrustState,
) -> (bool, TrustState, bool) {
    let event_key = event_key(event);
    let key = format!(
        "{}:{event_key}:{group_index}:{handler_index}",
        state.key_source
    );
    let persisted = state
        .config
        .and_then(|config| config.get("hooks"))
        .and_then(|hooks| hooks.get("state"))
        .and_then(toml::Value::as_table)
        .and_then(|states| states.get(&key));
    let enabled = enabled
        && persisted
            .and_then(|entry| entry.get("enabled"))
            .and_then(toml::Value::as_bool)
            != Some(false);
    let current_hash = codex_hook_hash(&event_key, group, handler, handler_type);
    let trusted_hash = persisted
        .and_then(|entry| entry.get("trusted_hash"))
        .and_then(toml::Value::as_str);
    let hook_trust = match (current_hash.as_deref(), trusted_hash) {
        (Some(current), Some(trusted)) if current == trusted => TrustState::Trusted,
        _ => TrustState::Untrusted,
    };
    let trust = match (base_trust, hook_trust) {
        (TrustState::Untrusted, _) | (_, TrustState::Untrusted) => TrustState::Untrusted,
        (TrustState::Unknown, _) => TrustState::Unknown,
        (_, trust) => trust,
    };
    let approval_pending = enabled && hook_trust != TrustState::Trusted;
    (enabled, trust, approval_pending)
}

fn codex_hook_hash(
    event_key: &str,
    group: &Map<String, Value>,
    handler: &Map<String, Value>,
    handler_type: &str,
) -> Option<String> {
    let mut normalized_handler = Map::new();
    normalized_handler.insert("type".to_string(), Value::String(handler_type.to_string()));
    match handler_type {
        "command" => normalize_command(event_key, handler, &mut normalized_handler)?,
        "mcp_tool" => normalize_mcp_tool(handler, &mut normalized_handler)?,
        _ => return None,
    }
    let mut identity = Map::new();
    identity.insert(
        "event_name".to_string(),
        Value::String(event_key.to_string()),
    );
    if let Some(matcher) = group.get("matcher").and_then(Value::as_str) {
        identity.insert("matcher".to_string(), Value::String(matcher.to_string()));
    }
    identity.insert(
        "hooks".to_string(),
        Value::Array(vec![Value::Object(normalized_handler)]),
    );
    let serialized = serde_json::to_vec(&canonical_json(&Value::Object(identity))).ok()?;
    Some(format!("sha256:{:x}", Sha256::digest(serialized)))
}

fn normalize_command(
    event_key: &str,
    handler: &Map<String, Value>,
    normalized: &mut Map<String, Value>,
) -> Option<()> {
    let command = handler.get("command").and_then(Value::as_str)?;
    normalized.insert("command".to_string(), Value::String(command.to_string()));
    let default_timeout = if event_key == "session_end" { 1 } else { 600 };
    let mut timeout = handler
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(default_timeout)
        .max(1);
    if event_key == "session_end" {
        timeout = timeout.min(3);
    }
    normalized.insert("timeout".to_string(), Value::from(timeout));
    normalized.insert(
        "async".to_string(),
        Value::Bool(
            handler
                .get("async")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
    );
    copy_optional_string(handler, normalized, "statusMessage");
    if additional_context_allowed(event_key) {
        if let Some(limit) = handler
            .get("additionalContextLimit")
            .and_then(Value::as_u64)
            .filter(|limit| *limit != 2_500)
        {
            normalized.insert("additionalContextLimit".to_string(), Value::from(limit));
        }
    }
    Some(())
}

fn normalize_mcp_tool(
    handler: &Map<String, Value>,
    normalized: &mut Map<String, Value>,
) -> Option<()> {
    copy_required_string(handler, normalized, "server")?;
    copy_required_string(handler, normalized, "tool")?;
    normalized.insert(
        "input".to_string(),
        handler
            .get("input")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new())),
    );
    normalized.insert(
        "timeout".to_string(),
        Value::from(
            handler
                .get("timeout")
                .and_then(Value::as_u64)
                .unwrap_or(600)
                .max(1),
        ),
    );
    copy_optional_string(handler, normalized, "statusMessage");
    Some(())
}

fn copy_required_string(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    key: &str,
) -> Option<()> {
    let value = source.get(key).and_then(Value::as_str)?;
    target.insert(key.to_string(), Value::String(value.to_string()));
    Some(())
}

fn copy_optional_string(source: &Map<String, Value>, target: &mut Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key).and_then(Value::as_str) {
        target.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn additional_context_allowed(event_key: &str) -> bool {
    matches!(
        event_key,
        "pre_tool_use"
            | "post_tool_use"
            | "session_start"
            | "user_prompt_submit"
            | "subagent_start"
    )
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .filter_map(|key| {
                        map.get(key)
                            .map(|value| (key.clone(), canonical_json(value)))
                    })
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

fn event_key(event: &str) -> String {
    let mut key = String::new();
    for character in event.chars() {
        if character.is_ascii_uppercase() && !key.is_empty() {
            key.push('_');
        }
        key.push(character.to_ascii_lowercase());
    }
    key
}
