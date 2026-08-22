use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug)]
enum McpPolicyMatcher {
    Name(String),
    Command(Vec<String>),
    Url(String),
}

impl McpPolicyMatcher {
    fn from_value(value: &Value) -> Option<Self> {
        if let Some(name) = value.as_str() {
            return Some(Self::Name(name.to_string()));
        }
        let value = value.as_object()?;
        if let Some(name) = value.get("serverName").and_then(Value::as_str) {
            return Some(Self::Name(name.to_string()));
        }
        if let Some(command) = value.get("serverCommand").and_then(Value::as_array) {
            let command: Vec<_> = command
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            if !command.is_empty() {
                return Some(Self::Command(command));
            }
        }
        value
            .get("serverUrl")
            .and_then(Value::as_str)
            .map(|url| Self::Url(url.to_string()))
    }

    fn matches(&self, name: &str, config: &serde_json::Map<String, Value>) -> bool {
        match self {
            Self::Name(expected) => expected == name,
            Self::Command(expected) => {
                let Some(command) = config.get("command").and_then(Value::as_str) else {
                    return false;
                };
                let actual = std::iter::once(command)
                    .chain(
                        config
                            .get("args")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str),
                    )
                    .collect::<Vec<_>>();
                actual == expected.iter().map(String::as_str).collect::<Vec<_>>()
            }
            Self::Url(pattern) => config
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|url| wildcard_matches(pattern, url)),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct ClaudeManagedPolicy {
    pub(super) allow_managed_hooks_only: bool,
    pub(super) managed_hooks_enabled: Option<bool>,
    allowed_mcps: Option<Vec<McpPolicyMatcher>>,
    denied_mcps: Vec<McpPolicyMatcher>,
    pub(super) managed_mcp_exclusive: bool,
}

impl ClaudeManagedPolicy {
    pub(super) fn new(settings: Option<&Value>, managed_mcp_exclusive: bool) -> Self {
        Self {
            allow_managed_hooks_only: settings
                .and_then(|settings| settings.get("allowManagedHooksOnly"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            managed_hooks_enabled: settings
                .and_then(|settings| settings.get("disableAllHooks"))
                .and_then(Value::as_bool)
                .map(|disabled| !disabled),
            allowed_mcps: policy_matchers(settings, "allowedMcpServers"),
            denied_mcps: policy_matchers(settings, "deniedMcpServers").unwrap_or_default(),
            managed_mcp_exclusive,
        }
    }

    pub(super) fn blocked_names(
        &self,
        servers: &serde_json::Map<String, Value>,
    ) -> HashSet<String> {
        servers
            .iter()
            .filter_map(|(name, value)| {
                let config = value.as_object()?;
                let denied = self
                    .denied_mcps
                    .iter()
                    .any(|matcher| matcher.matches(name, config));
                let allowed = self.allowed_mcps.as_ref().is_none_or(|matchers| {
                    matchers.iter().any(|matcher| matcher.matches(name, config))
                });
                (denied || !allowed).then(|| name.clone())
            })
            .collect()
    }
}

fn policy_matchers(settings: Option<&Value>, key: &str) -> Option<Vec<McpPolicyMatcher>> {
    settings?.get(key)?.as_array().map(|values| {
        values
            .iter()
            .filter_map(McpPolicyMatcher::from_value)
            .collect()
    })
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let parts: Vec<_> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == value;
    }
    let mut remainder = value;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(position) = remainder.find(part) else {
            return false;
        };
        if index == 0 && position != 0 {
            return false;
        }
        remainder = &remainder[position + part.len()..];
    }
    pattern.ends_with('*') || remainder.is_empty()
}
