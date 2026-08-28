use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    PluginConfig,
    PluginSkills,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustState {
    NotApplicable,
    Unknown,
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionBlockedReason {
    AlreadyEnabled,
    AlreadyDisabled,
    StateUnavailable,
    ManagedSource,
    AdministratorSource,
    PolicyControlled,
    PluginOwnedSource,
    MalformedSource,
    BrokenSymlink,
    UnsupportedByClient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReloadGuidance {
    NotRequired,
    RestartClient,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionAvailability {
    pub available: bool,
    pub blocked_reason: Option<ActionBlockedReason>,
}

impl ActionAvailability {
    fn available() -> Self {
        Self {
            available: true,
            blocked_reason: None,
        }
    }

    fn blocked(reason: ActionBlockedReason) -> Self {
        Self {
            available: false,
            blocked_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryActionCapabilities {
    pub enable: ActionAvailability,
    pub disable: ActionAvailability,
    pub confirmation_required: bool,
    pub reload_guidance: ReloadGuidance,
    pub source_revision: Option<String>,
}

impl InventoryActionCapabilities {
    /// Reports native actions for a record whose client has a documented state control.
    pub fn stateful(
        enabled: Option<bool>,
        confirmation_required: bool,
        reload_guidance: ReloadGuidance,
        source_revision: String,
    ) -> Self {
        let (enable, disable) = match enabled {
            Some(true) => (
                ActionAvailability::blocked(ActionBlockedReason::AlreadyEnabled),
                ActionAvailability::available(),
            ),
            Some(false) => (
                ActionAvailability::available(),
                ActionAvailability::blocked(ActionBlockedReason::AlreadyDisabled),
            ),
            None => (
                ActionAvailability::blocked(ActionBlockedReason::StateUnavailable),
                ActionAvailability::blocked(ActionBlockedReason::StateUnavailable),
            ),
        };
        let confirmation_required =
            confirmation_required && (enable.available || disable.available);
        Self {
            enable,
            disable,
            confirmation_required,
            reload_guidance,
            source_revision: Some(source_revision),
        }
    }

    /// Reports native actions when an enabled definition is waiting for client approval.
    pub fn pending_approval(
        confirmation_required: bool,
        reload_guidance: ReloadGuidance,
        source_revision: String,
    ) -> Self {
        Self {
            enable: ActionAvailability::available(),
            disable: ActionAvailability::available(),
            confirmation_required,
            reload_guidance,
            source_revision: Some(source_revision),
        }
    }

    /// Reports why neither native action can be offered safely.
    pub fn blocked(reason: ActionBlockedReason, source_revision: Option<String>) -> Self {
        Self {
            enable: ActionAvailability::blocked(reason),
            disable: ActionAvailability::blocked(reason),
            confirmation_required: false,
            reload_guidance: ReloadGuidance::NotRequired,
            source_revision,
        }
    }
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
    pub action_capabilities: InventoryActionCapabilities,
    #[serde(skip)]
    pub(crate) action_restriction: Option<ActionBlockedReason>,
    #[serde(skip)]
    pub(crate) approval_pending: bool,
    #[serde(skip)]
    pub(crate) codex_hook_state_key: Option<String>,
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
            action_capabilities: InventoryActionCapabilities::blocked(
                ActionBlockedReason::UnsupportedByClient,
                None,
            ),
            action_restriction: None,
            approval_pending: false,
            codex_hook_state_key: None,
        }
    }

    /// Marks a discovered record read-only for a normalized, non-sensitive reason.
    pub(crate) fn restrict_actions(&mut self, reason: ActionBlockedReason) {
        if self.action_restriction.is_none() {
            self.action_restriction = Some(reason);
        }
    }
}

/// Returns restrictions shared by all client adapters before client-native checks.
pub fn source_action_blocker(record: &InventoryRecord) -> Option<ActionBlockedReason> {
    if record.source_kind == SourceKind::ManagedConfig {
        return Some(ActionBlockedReason::ManagedSource);
    }
    if record.scope == InventoryScope::Admin || record.source_kind == SourceKind::AdminSkills {
        return Some(ActionBlockedReason::AdministratorSource);
    }
    if matches!(
        record.source_kind,
        SourceKind::PluginConfig | SourceKind::PluginSkills
    ) {
        return Some(ActionBlockedReason::PluginOwnedSource);
    }
    if record.is_symlink && record.resolved_path.is_none() {
        return Some(ActionBlockedReason::BrokenSymlink);
    }
    if let Some(reason) = record.action_restriction {
        return Some(reason);
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryWarning {
    pub client: Option<ClientKind>,
    pub source_path: String,
    pub message: String,
    pub blocked_reason: Option<ActionBlockedReason>,
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
            blocked_reason: None,
        }
    }

    /// Creates a warning whose safe reason can be translated by the interface.
    pub fn blocked(
        client: ClientKind,
        source_path: impl Into<String>,
        message: impl Into<String>,
        blocked_reason: ActionBlockedReason,
    ) -> Self {
        Self {
            client: Some(client),
            source_path: source_path.into(),
            message: message.into(),
            blocked_reason: Some(blocked_reason),
        }
    }

    pub fn general(source_path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            client: None,
            source_path: source_path.into(),
            message: message.into(),
            blocked_reason: None,
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
    #[serde(skip)]
    source_revisions: HashMap<String, SourceRevisionState>,
}

#[derive(Debug, Clone)]
struct SourceRevisionState {
    marker: String,
    restriction: Option<ActionBlockedReason>,
}

impl InventorySnapshot {
    pub fn new(scanned_project_count: usize) -> Self {
        Self {
            records: Vec::new(),
            warnings: Vec::new(),
            capabilities: Vec::new(),
            scanned_project_count,
            source_revisions: HashMap::new(),
        }
    }

    /// Stores a revision from the exact bytes used to parse a discovered source.
    pub(crate) fn record_source_revision(&mut self, path: &Path, content: &[u8]) {
        self.source_revisions.insert(
            path.display().to_string(),
            SourceRevisionState {
                marker: format!("present:sha256:{:x}", Sha256::digest(content)),
                restriction: None,
            },
        );
    }

    /// Stores that a native state source was absent when discovery read it.
    pub(crate) fn record_source_absence(&mut self, path: &Path) {
        self.source_revisions
            .entry(path.display().to_string())
            .or_insert_with(|| SourceRevisionState {
                marker: "absent".to_string(),
                restriction: None,
            });
    }

    /// Marks a source unsafe while retaining its observed fingerprint state.
    pub(crate) fn restrict_source(&mut self, path: &str, reason: ActionBlockedReason) {
        let state = self
            .source_revisions
            .entry(path.to_string())
            .or_insert_with(|| SourceRevisionState {
                marker: "unobserved".to_string(),
                restriction: None,
            });
        if state.restriction.is_none() {
            state.restriction = Some(reason);
        }
    }

    /// Builds one opaque revision across every source that owns an action's state.
    pub(crate) fn composite_source_revision(
        &self,
        source_paths: &[String],
    ) -> (String, Option<ActionBlockedReason>) {
        let mut source_paths = source_paths.to_vec();
        source_paths.sort();
        source_paths.dedup();
        let mut hasher = Sha256::new();
        let mut restriction = None;
        for source_path in source_paths {
            let state = self.source_revisions.get(&source_path);
            let marker = state.map_or("unobserved", |state| state.marker.as_str());
            if restriction.is_none() {
                restriction = state.and_then(|state| state.restriction).or_else(|| {
                    (!self.source_revisions.contains_key(&source_path))
                        .then_some(ActionBlockedReason::StateUnavailable)
                });
            }
            hash_revision_part(&mut hasher, source_path.as_bytes());
            hash_revision_part(&mut hasher, marker.as_bytes());
        }
        (format!("sha256:{:x}", hasher.finalize()), restriction)
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

fn hash_revision_part(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
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
