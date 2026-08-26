use super::actions::InventoryActionError;
use super::models::InventoryRecord;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{value, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

pub(super) fn update_codex_skill_config(
    original: &[u8],
    record: &InventoryRecord,
    enabled: bool,
) -> Result<String, InventoryActionError> {
    let text =
        std::str::from_utf8(original).map_err(|_| InventoryActionError::MalformedConfiguration)?;
    let mut document = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>()
            .map_err(|_| InventoryActionError::MalformedConfiguration)?
    };
    let skill_folder = Path::new(&record.original_path)
        .parent()
        .ok_or(InventoryActionError::UnsupportedRecord)?;
    let aliases = skill_path_aliases(record);

    let skills = document
        .entry("skills")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or(InventoryActionError::MalformedConfiguration)?;
    let entries = skills
        .entry("config")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()));
    let match_count = match entries {
        Item::ArrayOfTables(tables) => update_array_of_tables(tables, &aliases, enabled)?,
        Item::Value(Value::Array(values)) => update_inline_tables(values, &aliases, enabled)?,
        _ => return Err(InventoryActionError::MalformedConfiguration),
    };
    if match_count > 1 {
        return Err(InventoryActionError::AmbiguousRecord);
    }
    if match_count == 0 {
        append_skill_entry(entries, skill_folder, enabled)?;
    }

    validate_rendered_config(document.to_string(), &aliases, enabled)
}

fn validate_rendered_config(
    rendered: String,
    aliases: &[PathBuf],
    enabled: bool,
) -> Result<String, InventoryActionError> {
    let parsed: toml::Value =
        toml::from_str(&rendered).map_err(|_| InventoryActionError::MalformedConfiguration)?;
    let matching_entries = parsed
        .get("skills")
        .and_then(|value| value.get("config"))
        .and_then(toml::Value::as_array)
        .ok_or(InventoryActionError::MalformedConfiguration)?
        .iter()
        .filter(|entry| {
            entry
                .get("path")
                .and_then(toml::Value::as_str)
                .is_some_and(|path| configured_path_matches(Path::new(path), aliases))
                && entry.get("enabled").and_then(toml::Value::as_bool) == Some(enabled)
        })
        .count();
    if matching_entries != 1 {
        return Err(InventoryActionError::MalformedConfiguration);
    }
    Ok(rendered)
}

fn update_array_of_tables(
    tables: &mut ArrayOfTables,
    aliases: &[PathBuf],
    enabled: bool,
) -> Result<usize, InventoryActionError> {
    let mut matches = 0;
    for table in tables.iter_mut() {
        let configured_path = table
            .get("path")
            .and_then(Item::as_str)
            .filter(|path| !path.trim().is_empty())
            .ok_or(InventoryActionError::MalformedConfiguration)?;
        if table
            .get("enabled")
            .is_some_and(|item| item.as_bool().is_none())
        {
            return Err(InventoryActionError::MalformedConfiguration);
        }
        if configured_path_matches(Path::new(configured_path), aliases) {
            table["enabled"] = value(enabled);
            matches += 1;
        }
    }
    Ok(matches)
}

fn update_inline_tables(
    values: &mut toml_edit::Array,
    aliases: &[PathBuf],
    enabled: bool,
) -> Result<usize, InventoryActionError> {
    let mut matches = 0;
    for value in values.iter_mut() {
        let table = value
            .as_inline_table_mut()
            .ok_or(InventoryActionError::MalformedConfiguration)?;
        let configured_path = table
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .ok_or(InventoryActionError::MalformedConfiguration)?;
        if table
            .get("enabled")
            .is_some_and(|value| value.as_bool().is_none())
        {
            return Err(InventoryActionError::MalformedConfiguration);
        }
        if configured_path_matches(Path::new(configured_path), aliases) {
            table.insert("enabled", Value::from(enabled));
            matches += 1;
        }
    }
    Ok(matches)
}

fn append_skill_entry(
    entries: &mut Item,
    skill_folder: &Path,
    enabled: bool,
) -> Result<(), InventoryActionError> {
    let path = skill_folder.display().to_string();
    match entries {
        Item::ArrayOfTables(tables) => {
            let mut table = Table::new();
            table["path"] = value(path);
            table["enabled"] = value(enabled);
            tables.push(table);
            Ok(())
        }
        Item::Value(Value::Array(values)) => {
            let mut table = InlineTable::new();
            table.insert("path", Value::from(path));
            table.insert("enabled", Value::from(enabled));
            values.push(Value::InlineTable(table));
            Ok(())
        }
        _ => Err(InventoryActionError::MalformedConfiguration),
    }
}

fn skill_path_aliases(record: &InventoryRecord) -> Vec<PathBuf> {
    let mut aliases = vec![PathBuf::from(&record.original_path)];
    if let Some(parent) = Path::new(&record.original_path).parent() {
        aliases.push(parent.to_path_buf());
    }
    if let Some(resolved) = record.resolved_path.as_ref() {
        aliases.push(PathBuf::from(resolved));
        if let Some(parent) = Path::new(resolved).parent() {
            aliases.push(parent.to_path_buf());
        }
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn configured_path_matches(configured: &Path, aliases: &[PathBuf]) -> bool {
    if aliases.iter().any(|alias| alias == configured) {
        return true;
    }
    if let Ok(resolved) = fs::canonicalize(configured) {
        if aliases.iter().any(|alias| alias == &resolved) {
            return true;
        }
        if resolved.is_dir() {
            return aliases
                .iter()
                .any(|alias| alias == &resolved.join("SKILL.md"));
        }
    }
    aliases
        .iter()
        .any(|alias| alias == &configured.join("SKILL.md"))
}
