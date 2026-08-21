use serde::Serialize;
use std::collections::HashSet;
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
        let project_mcp_records: HashSet<_> = self
            .records
            .iter()
            .filter(|record| {
                record.item_type == InventoryItemType::Mcp && record.project_path.is_some()
            })
            .map(|record| (record.client, record.name.clone()))
            .collect();
        for record in &mut self.records {
            if record.item_type == InventoryItemType::Mcp
                && record.project_path.is_none()
                && record.enabled != Some(false)
                && record.trust_state != TrustState::Untrusted
                && project_mcp_records.contains(&(record.client, record.name.clone()))
            {
                record.is_effective = None;
            }
        }
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

#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub home_dir: PathBuf,
    pub codex_home: PathBuf,
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
