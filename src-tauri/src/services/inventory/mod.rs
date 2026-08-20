mod claude;
mod codex;
mod config;
mod cursor;
mod hooks;
mod models;
mod skills;

pub use models::InventorySnapshot;

use claude::ClaudeAdapter;
use codex::CodexAdapter;
use cursor::CursorAdapter;
use models::{AdapterCapabilities, DiscoveryContext};
use std::collections::HashSet;
use std::path::PathBuf;

trait ClientAdapter {
    fn capabilities(&self) -> AdapterCapabilities;
    fn discover(&self, context: &DiscoveryContext, snapshot: &mut InventorySnapshot);
}

pub fn discover_inventory(home_dir: PathBuf, project_roots: Vec<PathBuf>) -> InventorySnapshot {
    let project_roots = existing_unique_roots(project_roots);
    let context = DiscoveryContext {
        home_dir,
        project_roots,
    };
    let mut snapshot = InventorySnapshot::new(context.project_roots.len());
    let adapters: [&dyn ClientAdapter; 3] = [&ClaudeAdapter, &CodexAdapter, &CursorAdapter];
    for adapter in adapters {
        snapshot.capabilities.push(adapter.capabilities());
        adapter.discover(&context, &mut snapshot);
    }
    snapshot.finish()
}

fn existing_unique_roots(project_roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    for project_root in project_roots {
        if !project_root.is_dir() {
            continue;
        }
        let normalized = std::fs::canonicalize(&project_root).unwrap_or(project_root);
        if seen.insert(normalized.clone()) {
            roots.push(normalized);
        }
    }
    roots.sort();
    roots
}

#[cfg(test)]
mod tests;
