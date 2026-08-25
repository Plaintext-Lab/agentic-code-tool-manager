use super::super::models::{ClientKind, DiscoveryContext, InventorySnapshot, InventoryWarning};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) fn resolve_plugin_root(
    context: &DiscoveryContext,
    config: Option<&toml::Value>,
    marketplace_name: &str,
    plugin_name: &str,
    snapshot: &mut InventorySnapshot,
) -> Option<PathBuf> {
    let marketplace_root = configured_marketplace_root(config, marketplace_name)
        .filter(|path| path.is_dir())
        .or_else(|| cached_marketplace_root(context, marketplace_name, plugin_name));
    if let Some(root) = marketplace_root.as_ref() {
        if let Some(plugin_root) = plugin_root_from_marketplace(root, plugin_name) {
            return Some(plugin_root);
        }
    }
    let version = marketplace_root
        .as_ref()
        .and_then(|root| marketplace_plugin_version(root, plugin_name));
    if let Some(plugin_root) =
        cached_plugin_root(context, marketplace_name, plugin_name, version.as_deref())
    {
        return Some(plugin_root);
    }
    snapshot.warnings.push(InventoryWarning::new(
        ClientKind::Codex,
        context.codex_home.display().to_string(),
        "Could not resolve an installed plugin's local files.",
    ));
    None
}

fn configured_marketplace_root(
    config: Option<&toml::Value>,
    marketplace_name: &str,
) -> Option<PathBuf> {
    let marketplace = config?
        .get("marketplaces")?
        .get(marketplace_name)?
        .as_table()?;
    if marketplace.get("source_type")?.as_str()? != "local" {
        return None;
    }
    Some(PathBuf::from(marketplace.get("source")?.as_str()?))
}

fn cached_marketplace_root(
    context: &DiscoveryContext,
    marketplace_name: &str,
    plugin_name: &str,
) -> Option<PathBuf> {
    [
        context
            .codex_home
            .join(".tmp/marketplaces")
            .join(marketplace_name),
        context
            .codex_home
            .join(".tmp/bundled-marketplaces")
            .join(marketplace_name),
        context.codex_home.join(".tmp/plugins"),
    ]
    .into_iter()
    .find(|path| path.is_dir() && marketplace_plugin(path, plugin_name).is_some())
}

fn plugin_root_from_marketplace(root: &Path, plugin_name: &str) -> Option<PathBuf> {
    let manifest = read_json(&marketplace_manifest(root)?)?;
    let plugin = marketplace_plugin_value(&manifest, plugin_name)?;
    let source = plugin.get("source")?;
    let path = source
        .as_str()
        .or_else(|| source.as_object()?.get("path").and_then(Value::as_str))?;
    let candidate = root.join(path);
    candidate.is_dir().then_some(candidate)
}

fn marketplace_plugin_version(root: &Path, plugin_name: &str) -> Option<String> {
    marketplace_plugin(root, plugin_name)?
        .get("version")?
        .as_str()
        .map(str::to_string)
}

fn marketplace_plugin(root: &Path, plugin_name: &str) -> Option<Value> {
    let manifest = read_json(&marketplace_manifest(root)?)?;
    marketplace_plugin_value(&manifest, plugin_name).cloned()
}

fn marketplace_plugin_value<'a>(manifest: &'a Value, plugin_name: &str) -> Option<&'a Value> {
    manifest
        .get("plugins")?
        .as_array()?
        .iter()
        .find(|plugin| plugin.get("name").and_then(Value::as_str) == Some(plugin_name))
}

fn marketplace_manifest(root: &Path) -> Option<PathBuf> {
    [
        root.join(".agents/plugins/marketplace.json"),
        root.join(".codex-plugin/marketplace.json"),
        root.join(".claude-plugin/marketplace.json"),
        root.join("marketplace.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn cached_plugin_root(
    context: &DiscoveryContext,
    marketplace_name: &str,
    plugin_name: &str,
    version: Option<&str>,
) -> Option<PathBuf> {
    let cache_root = context
        .codex_home
        .join("plugins/cache")
        .join(marketplace_name)
        .join(plugin_name);
    if let Some(version) = version {
        let exact = cache_root.join(version);
        if exact.is_dir() {
            return Some(exact);
        }
    }
    let mut versions: Vec<_> = std::fs::read_dir(cache_root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    versions.sort();
    versions.pop()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}
