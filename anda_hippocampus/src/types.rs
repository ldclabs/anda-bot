use anda_core::{BoxError, Principal, Usage, model::Message};
use anda_db::storage::StorageStats;
use ic_cose_types::cose::cwt::{ClaimsSet, get_scope};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Deserialize)]
pub struct Pagination {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    /// Conversation collection: `"recall"`
    pub collection: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct SpaceStatus {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub owner: String,
    pub db_stats: StorageStats,
    pub concepts: usize,
    pub propositions: usize,
    pub conversations: usize,
    pub public: bool,
    pub tier: SpaceTier,
    pub formation_usage: Usage,
    pub recall_usage: Usage,
    pub maintenance_usage: Usage,
    pub formation_processed_id: u64,
}

pub struct CWToken {
    pub user: Principal,
    pub audience: String,
    pub scope: TokenScope,
}

impl CWToken {
    pub fn from_claims(claims: ClaimsSet) -> Result<Self, BoxError> {
        let scope = TokenScope::from_str(&get_scope(&claims).unwrap_or_default())?;
        let user = claims
            .subject
            .ok_or("missing 'sub' claim")?
            .parse::<Principal>()
            .map_err(|_| "invalid 'sub' claim")?;

        let audience = claims.audience.unwrap_or_default();
        Ok(Self {
            user,
            audience,
            scope,
        })
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct ModelConfig {
    // "gemini", "anthropic", "openai", "deepseek", "mimo" etc.
    #[serde(alias = "f")]
    pub family: String,

    #[serde(alias = "m")]
    pub model: String,

    #[serde(alias = "ab")]
    pub api_base: String,

    #[serde(alias = "ak")]
    pub api_key: String,

    #[serde(default, alias = "d")]
    pub disabled: bool,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct ModelConfigRef<'a> {
    #[serde(rename = "f", alias = "family")]
    pub family: &'a str,

    #[serde(rename = "m", alias = "model")]
    pub model: &'a str,

    #[serde(rename = "ab", alias = "api_base")]
    pub api_base: &'a str,

    #[serde(rename = "ak", alias = "api_key")]
    pub api_key: &'a str,

    #[serde(rename = "d", alias = "disabled")]
    pub disabled: bool,
}

impl ModelConfig {
    pub fn to_ref<'a>(&'a self) -> ModelConfigRef<'a> {
        ModelConfigRef {
            family: &self.family,
            model: &self.model,
            api_base: &self.api_base,
            api_key: &self.api_key,
            disabled: self.disabled,
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct SpaceTier {
    #[serde(default, alias = "t")]
    pub tier: u32,

    #[serde(default, alias = "u")]
    pub updated_at: u64,
}

impl SpaceTier {
    pub fn to_ref(&self) -> SpaceTierRef {
        SpaceTierRef {
            tier: self.tier,
            updated_at: self.updated_at,
        }
    }

    // tier 0 (free) allows 100 nodes, tier 1 allows 1k, etc.
    pub fn allow_nodes(&self) -> u64 {
        10u64.pow(self.tier + 2)
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct SpaceTierRef {
    #[serde(rename = "t", alias = "tier")]
    pub tier: u32,
    #[serde(rename = "u", alias = "updated_at")]
    pub updated_at: u64,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct SpaceToken {
    #[serde(default, alias = "n")]
    pub name: String,

    #[serde(default)]
    pub token: String,

    #[serde(alias = "s")]
    pub scope: TokenScope,

    #[serde(default, alias = "u")]
    pub usage: u64,

    #[serde(default, alias = "ca")]
    pub created_at: u64,

    #[serde(default, alias = "ua")]
    pub updated_at: u64,

    #[serde(default, alias = "ea")]
    pub expires_at: Option<u64>,
}

impl SpaceToken {
    pub fn to_ref<'a>(&'a self) -> SpaceTokenRef<'a> {
        SpaceTokenRef {
            name: &self.name,
            scope: &self.scope,
            usage: self.usage,
            created_at: self.created_at,
            updated_at: self.updated_at,
            expires_at: self.expires_at,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct SpaceTokenRef<'a> {
    #[serde(rename = "n", alias = "name")]
    pub name: &'a str,
    #[serde(rename = "s", alias = "scope")]
    pub scope: &'a TokenScope,
    #[serde(rename = "u", alias = "usage")]
    pub usage: u64,
    #[serde(rename = "ca", alias = "created_at")]
    pub created_at: u64,
    #[serde(rename = "ua", alias = "updated_at")]
    pub updated_at: u64,
    #[serde(rename = "ea", alias = "expires_at")]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum TokenScope {
    #[serde(rename = "read")]
    #[default]
    Read,
    #[serde(rename = "write")]
    Write,
    #[serde(rename = "*")]
    All,
}

impl TokenScope {
    pub fn allows(&self, required: Self) -> bool {
        *self == Self::All || *self == required
    }
}

impl FromStr for TokenScope {
    type Err = BoxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "*" => Ok(Self::All),
            _ => Err("invalid scope".into()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AddSpaceTokenInput {
    pub scope: TokenScope,
    #[serde(default)]
    pub name: String,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RevokeSpaceTokenInput {
    pub token: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UpdateSpaceInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub public: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct InputContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct RecallInput {
    pub query: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<InputContext>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct FormationInput {
    pub messages: Vec<Message>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<InputContext>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct FormationRestartInput {
    pub conversation: u64,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct MaintenanceInput {
    /// `"scheduled"` | `"threshold"` | `"on_demand"`
    #[serde(default = "default_trigger")]
    pub trigger: String,

    /// `"full"` (complete maintenance cycle) | `"quick"` (lightweight check only)
    #[serde(default = "default_scope")]
    pub scope: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<MaintenanceParameters>,
}

fn default_trigger() -> String {
    "on_demand".to_string()
}

fn default_scope() -> String {
    "full".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MaintenanceParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_event_threshold_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_decay_factor: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsorted_max_backlog: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orphan_max_count: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateOrUpdateSpaceInput {
    pub user: Principal,
    pub space_id: String,
    pub tier: u32,
}

#[cfg(test)]
mod tests {
    use super::{SpaceToken, TokenScope};
    use std::str::FromStr;

    #[test]
    fn space_token_scope_serde_roundtrip() {
        let read = serde_json::to_string(&TokenScope::Read).unwrap();
        let write = serde_json::to_string(&TokenScope::Write).unwrap();
        let all = serde_json::to_string(&TokenScope::All).unwrap();

        assert_eq!(read, "\"read\"");
        assert_eq!(write, "\"write\"");
        assert_eq!(all, "\"*\"");

        assert_eq!(
            serde_json::from_str::<TokenScope>("\"read\"").unwrap(),
            TokenScope::Read
        );
        assert_eq!(
            serde_json::from_str::<TokenScope>("\"write\"").unwrap(),
            TokenScope::Write
        );
        assert_eq!(
            serde_json::from_str::<TokenScope>("\"*\"").unwrap(),
            TokenScope::All
        );
    }

    #[test]
    fn space_token_scope_from_str_and_allows() {
        assert_eq!(TokenScope::from_str("read").unwrap(), TokenScope::Read);
        assert_eq!(TokenScope::from_str("write").unwrap(), TokenScope::Write);
        assert_eq!(TokenScope::from_str("*").unwrap(), TokenScope::All);
        assert!(TokenScope::All.allows(TokenScope::Read));
        assert!(TokenScope::All.allows(TokenScope::Write));
        assert!(TokenScope::Read.allows(TokenScope::Read));
        assert!(!TokenScope::Read.allows(TokenScope::Write));
        assert!(TokenScope::from_str("unknown").is_err());
    }

    #[test]
    fn space_token_deserialize_accepts_verbose_and_compact_fields() {
        let verbose = r#"{"scope":"write","usage":3,"created_at":11,"updated_at":12}"#;
        let compact = r#"{"s":"read","u":7,"ca":21,"ua":22}"#;

        let verbose_token: SpaceToken = serde_json::from_str(verbose).unwrap();
        assert_eq!(verbose_token.scope, TokenScope::Write);
        assert_eq!(verbose_token.usage, 3);
        assert_eq!(verbose_token.created_at, 11);
        assert_eq!(verbose_token.updated_at, 12);

        let compact_token: SpaceToken = serde_json::from_str(compact).unwrap();
        assert_eq!(compact_token.scope, TokenScope::Read);
        assert_eq!(compact_token.usage, 7);
        assert_eq!(compact_token.created_at, 21);
        assert_eq!(compact_token.updated_at, 22);
    }

    #[test]
    fn space_token_serialize_uses_verbose_field_names() {
        let token = SpaceToken {
            token: "abc123".to_string(),
            scope: TokenScope::Write,
            usage: 9,
            created_at: 101,
            updated_at: 102,
            ..Default::default()
        };

        let value = serde_json::to_value(&token).unwrap();
        assert_eq!(value["scope"], "write");
        assert_eq!(value["usage"], 9);
        assert_eq!(value["created_at"], 101);
        assert_eq!(value["updated_at"], 102);
        assert!(value.get("s").is_none());
        assert!(value.get("u").is_none());
        assert!(value.get("ca").is_none());
        assert!(value.get("ua").is_none());
    }
}
