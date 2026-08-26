mod claude;
mod claude_policy;
mod codex;
mod config;
mod cursor;
mod hooks;
mod models;
mod plugins;
mod skills;

pub use models::InventorySnapshot;

use claude::ClaudeAdapter;
use codex::CodexAdapter;
use cursor::CursorAdapter;
use models::{
    ActionBlockedReason, AdapterCapabilities, DiscoveryContext, InventoryActionCapabilities,
    InventoryRecord, InventoryWarning,
};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

trait ClientAdapter {
    fn capabilities(&self) -> AdapterCapabilities;
    fn discover(&self, context: &DiscoveryContext, snapshot: &mut InventorySnapshot);
    fn action_revision_sources(
        &self,
        context: &DiscoveryContext,
        record: &InventoryRecord,
    ) -> Vec<String>;
    fn action_capabilities(
        &self,
        record: &InventoryRecord,
        source_revision: String,
    ) -> InventoryActionCapabilities;
}

pub fn discover_inventory(home_dir: PathBuf, project_roots: Vec<PathBuf>) -> InventorySnapshot {
    let codex_home = codex_home_path(&home_dir, std::env::var_os("CODEX_HOME"));
    discover_inventory_with_codex_home(home_dir, codex_home, project_roots)
}

fn codex_home_path(home_dir: &Path, configured_home: Option<OsString>) -> PathBuf {
    configured_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir.join(".codex"))
}

fn discover_inventory_with_codex_home(
    home_dir: PathBuf,
    codex_home: PathBuf,
    project_roots: Vec<PathBuf>,
) -> InventorySnapshot {
    let managed_settings_path = super::managed_settings::managed_settings_path();
    let managed_mcp_path = managed_settings_path.with_file_name("managed-mcp.json");
    discover_inventory_with_paths(
        home_dir,
        codex_home,
        managed_settings_path,
        managed_mcp_path,
        project_roots,
    )
}

fn discover_inventory_with_paths(
    home_dir: PathBuf,
    codex_home: PathBuf,
    claude_managed_settings_path: PathBuf,
    claude_managed_mcp_path: PathBuf,
    project_roots: Vec<PathBuf>,
) -> InventorySnapshot {
    let (project_roots, warnings) = existing_unique_roots(project_roots);
    let context = DiscoveryContext {
        home_dir,
        codex_home,
        claude_managed_settings_path,
        claude_managed_mcp_path,
        project_roots,
    };
    let mut snapshot = InventorySnapshot::new(context.project_roots.len());
    snapshot.warnings = warnings;
    let adapters: [&dyn ClientAdapter; 3] = [&ClaudeAdapter, &CodexAdapter, &CursorAdapter];
    for adapter in adapters {
        snapshot.capabilities.push(adapter.capabilities());
        let first_new_record = snapshot.records.len();
        let first_new_warning = snapshot.warnings.len();
        adapter.discover(&context, &mut snapshot);
        let source_restrictions: Vec<_> = snapshot.warnings[first_new_warning..]
            .iter()
            .filter_map(|warning| {
                Some((
                    warning.client?,
                    warning.source_path.clone(),
                    warning
                        .blocked_reason
                        .unwrap_or(ActionBlockedReason::MalformedSource),
                ))
            })
            .collect();
        for (_, source_path, reason) in &source_restrictions {
            snapshot.restrict_source(source_path, *reason);
        }
        let source_revisions: Vec<_> = snapshot.records[first_new_record..]
            .iter()
            .map(|record| {
                let sources = adapter.action_revision_sources(&context, record);
                snapshot.composite_source_revision(&sources)
            })
            .collect();
        let mut unavailable_revisions = HashSet::new();
        for (record, (source_revision, revision_restriction)) in snapshot.records
            [first_new_record..]
            .iter_mut()
            .zip(source_revisions)
        {
            if let Some(reason) = revision_restriction {
                record.restrict_actions(reason);
            }
            if revision_restriction == Some(ActionBlockedReason::StateUnavailable) {
                unavailable_revisions.insert((record.client, record.source_path.clone()));
            }
            record.action_capabilities = adapter.action_capabilities(record, source_revision);
        }
        snapshot.warnings.extend(
            unavailable_revisions
                .into_iter()
                .map(|(client, source_path)| {
                    InventoryWarning::blocked(
                        client,
                        source_path,
                        "Could not create a source revision; actions remain read-only.",
                        ActionBlockedReason::StateUnavailable,
                    )
                }),
        );
    }
    snapshot.finish()
}

fn existing_unique_roots(project_roots: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<InventoryWarning>) {
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    let mut warnings = Vec::new();
    for project_root in project_roots {
        if !project_root.is_dir() {
            warnings.push(InventoryWarning::general(
                project_root.display().to_string(),
                "This registered project could not be scanned because its folder is unavailable.",
            ));
            continue;
        }
        let normalized = std::fs::canonicalize(&project_root).unwrap_or(project_root);
        if seen.insert(normalized.clone()) {
            roots.push(normalized);
        }
    }
    roots.sort();
    (roots, warnings)
}

#[cfg(test)]
mod tests;
