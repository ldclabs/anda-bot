use anda_core::{BoxError, Principal, model::Message};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

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
