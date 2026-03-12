use anda_core::{BoxError, Principal, model::Message};
use ic_cose_types::cose::cwt::{ClaimsSet, get_scope};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Deserialize)]
pub struct Pagination {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
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

#[derive(Debug, Clone)]
pub struct SpaceId {
    pub id: String,
    pub sharding: u32,
}

impl fmt::Display for SpaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s{}-{}", self.sharding, self.id)
    }
}

impl FromStr for SpaceId {
    type Err = BoxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(2, '-').collect();
        if parts.len() != 2 || !parts[0].starts_with('s') {
            return Err("invalid space id format".into());
        }
        let sharding = parts[0][1..].parse::<u32>()?;
        let id = parts[1].to_string();
        Ok(SpaceId { id, sharding })
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct SpaceToken {
    #[serde(alias = "s")]
    pub scope: TokenScope,

    #[serde(default, alias = "u")]
    pub usage: u64,

    #[serde(default, alias = "ca")]
    pub created_at: u64,

    #[serde(default, alias = "ua")]
    pub updated_at: u64,
}

impl SpaceToken {
    pub fn to_ref(&self) -> SpaceTokenRef {
        SpaceTokenRef {
            scope: self.scope,
            usage: self.usage,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct SpaceTokenRef {
    #[serde(rename = "s", alias = "scope")]
    pub scope: TokenScope,
    #[serde(rename = "u", alias = "usage")]
    pub usage: u64,
    #[serde(rename = "ca", alias = "created_at")]
    pub created_at: u64,
    #[serde(rename = "ua", alias = "updated_at")]
    pub updated_at: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
pub enum TokenScope {
    #[serde(rename = "read")]
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

impl Default for TokenScope {
    fn default() -> Self {
        Self::Read
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
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RevokeSpaceTokenInput {
    pub token: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SetSpacePublicInput {
    pub public: bool,
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
    pub timestamp: String,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct MaintenanceInput {
    /// `"scheduled"` | `"threshold"` | `"on_demand"`
    #[serde(default = "default_trigger")]
    pub trigger: String,

    /// `"full"` (complete maintenance cycle) | `"quick"` (lightweight check only)
    #[serde(default = "default_scope")]
    pub scope: String,

    pub timestamp: String,

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
pub struct CreateSpaceInput {
    pub user: Principal,
    pub space_id: String,
}

#[cfg(test)]
mod tests {
    use super::{SpaceToken, SpaceTokenRef, TokenScope};
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
            scope: TokenScope::Write,
            usage: 9,
            created_at: 101,
            updated_at: 102,
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

    #[test]
    fn space_token_ref_serialize_uses_compact_field_names() {
        let token = SpaceToken {
            scope: TokenScope::All,
            usage: 5,
            created_at: 200,
            updated_at: 201,
        };

        let token_ref = token.to_ref();
        let value = serde_json::to_value(&token_ref).unwrap();
        assert_eq!(value["s"], "*");
        assert_eq!(value["u"], 5);
        assert_eq!(value["ca"], 200);
        assert_eq!(value["ua"], 201);
        assert!(value.get("scope").is_none());
        assert!(value.get("usage").is_none());
        assert!(value.get("created_at").is_none());
        assert!(value.get("updated_at").is_none());
    }

    #[test]
    fn space_token_ref_deserialize_accepts_compact_and_verbose_fields() {
        let compact = r#"{"s":"write","u":1,"ca":2,"ua":3}"#;
        let verbose = r#"{"scope":"read","usage":4,"created_at":5,"updated_at":6}"#;

        let compact_ref: SpaceTokenRef = serde_json::from_str(compact).unwrap();
        assert_eq!(compact_ref.scope, TokenScope::Write);
        assert_eq!(compact_ref.usage, 1);
        assert_eq!(compact_ref.created_at, 2);
        assert_eq!(compact_ref.updated_at, 3);

        let verbose_ref: SpaceTokenRef = serde_json::from_str(verbose).unwrap();
        assert_eq!(verbose_ref.scope, TokenScope::Read);
        assert_eq!(verbose_ref.usage, 4);
        assert_eq!(verbose_ref.created_at, 5);
        assert_eq!(verbose_ref.updated_at, 6);
    }

    #[test]
    fn space_token_defaults_are_stable() {
        let token = SpaceToken::default();
        assert_eq!(token.scope, TokenScope::Read);
        assert_eq!(token.usage, 0);
        assert_eq!(token.created_at, 0);
        assert_eq!(token.updated_at, 0);

        let token_ref = SpaceTokenRef::default();
        assert_eq!(token_ref.scope, TokenScope::Read);
        assert_eq!(token_ref.usage, 0);
        assert_eq!(token_ref.created_at, 0);
        assert_eq!(token_ref.updated_at, 0);
    }
}
