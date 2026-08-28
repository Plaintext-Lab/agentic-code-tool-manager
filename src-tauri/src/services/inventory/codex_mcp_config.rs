use super::actions::{InventoryActionError, InventoryActionRequest};
use toml_edit::{value, DocumentMut, Item, Value};

pub(super) fn update_codex_mcp_config(
    original: &[u8],
    request: &InventoryActionRequest,
    server_name: &str,
) -> Result<String, InventoryActionError> {
    let text =
        std::str::from_utf8(original).map_err(|_| InventoryActionError::MalformedConfiguration)?;
    let mut document = text
        .parse::<DocumentMut>()
        .map_err(|_| InventoryActionError::MalformedConfiguration)?;
    let servers = document
        .get_mut("mcp_servers")
        .ok_or(InventoryActionError::MissingRecord)?;
    match servers {
        Item::Table(servers) => match servers
            .get_mut(server_name)
            .ok_or(InventoryActionError::MissingRecord)?
        {
            Item::Table(server) => server["enabled"] = value(request.enabled),
            Item::Value(Value::InlineTable(server)) => {
                server.insert("enabled", Value::from(request.enabled));
            }
            _ => return Err(InventoryActionError::MalformedConfiguration),
        },
        Item::Value(Value::InlineTable(servers)) => {
            let server = servers
                .get_mut(server_name)
                .and_then(Value::as_inline_table_mut)
                .ok_or(InventoryActionError::MissingRecord)?;
            server.insert("enabled", Value::from(request.enabled));
        }
        _ => return Err(InventoryActionError::MalformedConfiguration),
    }

    let rendered = document.to_string();
    let parsed: toml::Value =
        toml::from_str(&rendered).map_err(|_| InventoryActionError::MalformedConfiguration)?;
    let verified_enabled = parsed
        .get("mcp_servers")
        .and_then(|servers| servers.get(server_name))
        .and_then(|server| server.get("enabled"))
        .and_then(toml::Value::as_bool);
    if verified_enabled != Some(request.enabled) {
        return Err(InventoryActionError::MalformedConfiguration);
    }
    Ok(rendered)
}
