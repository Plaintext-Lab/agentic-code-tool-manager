use super::actions::InventoryActionError;
use super::models::InventoryRecord;
use toml_edit::{value, DocumentMut, InlineTable, Item, Table, Value};

pub(super) fn codex_hook_state_is_editable(config: Option<&toml::Value>, state_key: &str) -> bool {
    let Some(hooks) = config.and_then(|config| config.get("hooks")) else {
        return true;
    };
    let Some(state) = hooks.get("state") else {
        return true;
    };
    let Some(states) = state.as_table() else {
        return false;
    };
    let Some(entry) = states.get(state_key) else {
        return true;
    };
    let Some(entry) = entry.as_table() else {
        return false;
    };
    entry
        .get("enabled")
        .is_none_or(|enabled| enabled.as_bool().is_some())
        && entry
            .get("trusted_hash")
            .is_none_or(|trusted_hash| trusted_hash.as_str().is_some())
}

pub(super) fn update_codex_hook_config(
    original: &[u8],
    record: &InventoryRecord,
    enabled: bool,
) -> Result<String, InventoryActionError> {
    let state_key = record
        .codex_hook_state_key
        .as_deref()
        .ok_or(InventoryActionError::UnsupportedRecord)?;
    let text =
        std::str::from_utf8(original).map_err(|_| InventoryActionError::MalformedConfiguration)?;
    let original_config = parse_config(text)?;
    if !codex_hook_state_is_editable(Some(&original_config), state_key) {
        return Err(InventoryActionError::MalformedConfiguration);
    }
    let original_trusted_hash =
        hook_state_field(&original_config, state_key, "trusted_hash").cloned();
    let mut document = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>()
            .map_err(|_| InventoryActionError::MalformedConfiguration)?
    };

    let hooks = table_entry(document.as_table_mut(), "hooks")?;
    let states = table_entry(hooks, "state")?;
    let state = table_entry(states, state_key)?;
    if state
        .get("enabled")
        .is_some_and(|item| item.as_bool().is_none())
    {
        return Err(InventoryActionError::MalformedConfiguration);
    }
    state["enabled"] = value(enabled);

    validate_rendered_config(
        document.to_string(),
        state_key,
        enabled,
        original_trusted_hash,
    )
}

fn parse_config(text: &str) -> Result<toml::Value, InventoryActionError> {
    if text.trim().is_empty() {
        Ok(toml::Value::Table(toml::Table::new()))
    } else {
        toml::from_str(text).map_err(|_| InventoryActionError::MalformedConfiguration)
    }
}

fn table_entry<'a>(table: &'a mut Table, key: &str) -> Result<&'a mut Table, InventoryActionError> {
    let item = table.entry(key).or_insert(Item::Table(Table::new()));
    if let Item::Value(Value::InlineTable(inline)) = item {
        let mut replacement = InlineTable::new();
        std::mem::swap(inline, &mut replacement);
        *item = Item::Table(replacement.into_table());
    }
    item.as_table_mut()
        .ok_or(InventoryActionError::MalformedConfiguration)
}

fn hook_state_field<'a>(
    config: &'a toml::Value,
    state_key: &str,
    field: &str,
) -> Option<&'a toml::Value> {
    config
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        .and_then(|states| states.get(state_key))
        .and_then(|state| state.get(field))
}

fn validate_rendered_config(
    rendered: String,
    state_key: &str,
    enabled: bool,
    original_trusted_hash: Option<toml::Value>,
) -> Result<String, InventoryActionError> {
    let parsed: toml::Value =
        toml::from_str(&rendered).map_err(|_| InventoryActionError::MalformedConfiguration)?;
    if hook_state_field(&parsed, state_key, "enabled").and_then(toml::Value::as_bool)
        != Some(enabled)
        || hook_state_field(&parsed, state_key, "trusted_hash").cloned() != original_trusted_hash
    {
        return Err(InventoryActionError::MalformedConfiguration);
    }
    Ok(rendered)
}
