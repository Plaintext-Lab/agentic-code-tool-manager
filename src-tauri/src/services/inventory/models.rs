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
        ordinal: usize,
        source_priority: u16,
    ) -> Self {
        let id = format!(
            "{}:{}:{}:{}:{}",
            client.as_str(),
            item_type.as_str(),
            source_path,
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
            project_path: None,
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
    pub client: ClientKind,
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
            client,
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
            (left.client.as_str(), &left.source_path)
                .cmp(&(right.client.as_str(), &right.source_path))
        });
        self
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub home_dir: PathBuf,
    pub project_roots: Vec<PathBuf>,
}
