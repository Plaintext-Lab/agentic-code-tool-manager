use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    Claude,
    Codex,
    Cursor,
}

impl ClientKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InventoryItemType {
    Skill,
    Mcp,
    Hook,
}

impl InventoryItemType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Mcp => "mcp",
            Self::Hook => "hook",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InventoryScope {
    User,
    Project,
    Admin,
    Legacy,
}

impl InventoryScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Admin => "admin",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    UserConfig,
    ProjectConfig,
    LocalConfig,
    ManagedConfig,
    UserSkills,
    ProjectSkills,
    AdminSkills,
    LegacySkills,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustState {
    NotApplicable,
    Unknown,
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterCapabilities {
    pub client: ClientKind,
    pub skills: bool,
    pub mcps: bool,
    pub hooks: bool,
}

impl AdapterCapabilities {
    pub fn complete(client: ClientKind) -> Self {
        Self {
            client,
            skills: true,
            mcps: true,
            hooks: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryRecord {
    pub id: String,
    pub client: ClientKind,
    pub item_type: InventoryItemType,
    pub name: String,
    pub scope: InventoryScope,
    pub source_kind: SourceKind,
    pub source_path: String,
    pub project_path: Option<String>,
    pub original_path: String,
    pub resolved_path: Option<String>,
    pub is_symlink: bool,
    pub enabled: Option<bool>,
    pub trust_state: TrustState,
    pub is_effective: Option<bool>,
    pub source_priority: u16,
    pub protected_fields: Vec<String>,
    pub detail: Option<String>,
}

impl InventoryRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: ClientKind,
        item_type: InventoryItemType,
        name: String,
        scope: InventoryScope,
        source_kind: SourceKind,
        source_path: String,
        project_path: Option<&std::path::Path>,
        ordinal: usize,
        source_priority: u16,
    ) -> Self {
        let project_path = project_path.map(|path| path.display().to_string());
        let id = format!(
            "{}:{}:{}:{}:{}:{}:{}",
            client.as_str(),
            item_type.as_str(),
            scope.as_str(),
            source_path,
            project_path.as_deref().unwrap_or_default(),
            name,
            ordinal
        );
        Self {
            id,
            client,
            item_type,
            name,
            scope,
            source_kind,
            original_path: source_path.clone(),
            source_path,
            project_path,
            resolved_path: None,
            is_symlink: false,
            enabled: None,
            trust_state: TrustState::NotApplicable,
            is_effective: None,
            source_priority,
            protected_fields: Vec::new(),
            detail: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryWarning {
    pub client: Option<ClientKind>,
    pub source_path: String,
    pub message: String,
}

impl InventoryWarning {
    pub fn new(
        client: ClientKind,
        source_path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            client: Some(client),
            source_path: source_path.into(),
            message: message.into(),
        }
    }

    pub fn general(source_path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            client: None,
            source_path: source_path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySnapshot {
    pub records: Vec<InventoryRecord>,
    pub warnings: Vec<InventoryWarning>,
    pub capabilities: Vec<AdapterCapabilities>,
    pub scanned_project_count: usize,
}

impl InventorySnapshot {
    pub fn new(scanned_project_count: usize) -> Self {
        Self {
            records: Vec::new(),
            warnings: Vec::new(),
            capabilities: Vec::new(),
            scanned_project_count,
        }
    }

    pub fn finish(mut self) -> Self {
        reconcile_contextual_effectiveness(&mut self.records, InventoryItemType::Skill);
        reconcile_contextual_effectiveness(&mut self.records, InventoryItemType::Mcp);
        let mut seen = HashSet::new();
        self.records.retain(|record| seen.insert(record.id.clone()));
        self.records.sort_by(|left, right| {
            (
                left.client.as_str(),
                left.item_type.as_str(),
                left.name.to_lowercase(),
                &left.source_path,
            )
                .cmp(&(
                    right.client.as_str(),
                    right.item_type.as_str(),
                    right.name.to_lowercase(),
                    &right.source_path,
                ))
        });
        self.warnings.sort_by(|left, right| {
            (
                left.client.map(ClientKind::as_str).unwrap_or_default(),
                &left.source_path,
            )
                .cmp(&(
                    right.client.map(ClientKind::as_str).unwrap_or_default(),
                    &right.source_path,
                ))
        });
        self
    }
}

fn reconcile_contextual_effectiveness(
    records: &mut [InventoryRecord],
    item_type: InventoryItemType,
) {
    let groups: HashSet<_> = records
        .iter()
        .filter(|record| record.item_type == item_type)
        .map(|record| (record.client, record.name.clone()))
        .collect();
    for (client, name) in groups {
        let applicable_indices: Vec<_> = records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.client == client
                    && record.item_type == item_type
                    && record.name == name
                    && record.enabled.is_some()
                    && (record.enabled == Some(false) || record.is_effective != Some(false))
            })
            .map(|(index, _)| index)
            .collect();
        let global_indices: Vec<_> = applicable_indices
            .iter()
            .copied()
            .filter(|index| records[*index].project_path.is_none())
            .collect();
        let mut states = HashMap::new();
        apply_context_states(records, &global_indices, &mut states);

        let project_paths: HashSet<_> = applicable_indices
            .iter()
            .filter_map(|index| records[*index].project_path.clone())
            .collect();
        for project_path in project_paths {
            let mut contextual_indices = global_indices.clone();
            contextual_indices.extend(applicable_indices.iter().copied().filter(|index| {
                records[*index].project_path.as_deref() == Some(project_path.as_str())
            }));
            let context_states = context_states(records, &contextual_indices);
            for index in contextual_indices {
                let context_state = context_states[&index];
                if records[index].project_path.is_some() {
                    states.insert(index, context_state);
                } else if states.get(&index) == Some(&Some(true)) && context_state != Some(true) {
                    states.insert(index, None);
                }
            }
        }
        for (index, state) in states {
            records[index].is_effective = state;
        }
    }
}

fn apply_context_states(
    records: &[InventoryRecord],
    indices: &[usize],
    states: &mut HashMap<usize, Option<bool>>,
) {
    states.extend(context_states(records, indices));
}

fn context_states(records: &[InventoryRecord], indices: &[usize]) -> HashMap<usize, Option<bool>> {
    let Some(max_priority) = indices
        .iter()
        .map(|index| records[*index].source_priority)
        .max()
    else {
        return HashMap::new();
    };
    let winner_count = indices
        .iter()
        .filter(|index| records[**index].source_priority == max_priority)
        .count();
    indices
        .iter()
        .map(|index| {
            let state = if records[*index].source_priority < max_priority {
                Some(false)
            } else if winner_count == 1 {
                records[*index].is_effective
            } else {
                None
            };
            (*index, state)
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub home_dir: PathBuf,
    pub codex_home: PathBuf,
    pub claude_managed_settings_path: PathBuf,
    pub claude_managed_mcp_path: PathBuf,
    pub project_roots: Vec<PathBuf>,
}

pub fn effective_state(enabled: bool, trust_state: TrustState) -> Option<bool> {
    if !enabled || trust_state == TrustState::Untrusted {
        return Some(false);
    }
    match trust_state {
        TrustState::Unknown => None,
        TrustState::NotApplicable | TrustState::Trusted => Some(true),
        TrustState::Untrusted => Some(false),
    }
}
