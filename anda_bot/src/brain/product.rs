//! User-facing projections. A connected service is not proof that a fact was saved.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub budget: anda_brain::recall_budget::RecallBudget,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SearchResult {
    pub schema_version: u32,
    pub incomplete: bool,
    /// The complete bounded native packet, including coverage and warnings.
    pub packet: String,
    pub found: bool,
    pub conversation: Option<String>,
    pub budget: anda_brain::types::RecallBudgetReceipt,
}
use std::collections::BTreeMap;

pub const MEMORY_GUIDE: &str = include_str!("../../assets/memory-guide.txt");

pub fn source_identity(
    caller: &str,
    conversation: u64,
    session: Option<&str>,
) -> anda_brain::product::SourceIdentity {
    anda_brain::product::SourceIdentity {
        key: format!("anda-bot/conversation/{caller}/{conversation}"),
        parents: session
            .map(|session| vec![format!("anda-bot/session/{caller}/{session}")])
            .unwrap_or_default(),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadState {
    Reachable,
    Available,
    NotConfigured,
    Unauthorized,
    Forbidden,
    Unavailable,
    Timeout,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemoryStatus {
    pub state: ReadState,
    pub formation_active: Option<bool>,
    pub maintenance_active: Option<bool>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InboxStatus {
    pub state: ReadState,
    pub visible_items: Option<usize>,
    pub inventory_complete: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Available,
    NotConfigured,
    Unsupported,
    Forbidden,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Capability {
    pub state: CapabilityState,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WatchRequest {
    pub operation_id: String,
    pub record_id: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct WatchIntent {
    #[serde(default)]
    pub operation_id: String,
    pub caller: String,
    pub record_id: String,
    pub summary: String,
}

impl Capability {
    pub fn unsupported(reason: &str) -> Self {
        Self {
            state: CapabilityState::Unsupported,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemoryOverview {
    #[serde(default)]
    pub learning: serde_json::Value,
    pub caller: Option<String>,
    pub schema_version: u32,
    pub observed_at: u64,
    pub memory: MemoryStatus,
    pub inbox: InboxStatus,
    pub capabilities: BTreeMap<String, Capability>,
}

impl MemoryOverview {
    pub fn is_connected(&self) -> bool {
        self.memory.state == ReadState::Reachable
    }

    pub fn render(&self) -> String {
        let mut lines = vec!["Memory / 长期记忆".to_string()];
        if self.is_connected() {
            lines.push("Memory service connected / 记忆服务已连接".into());
            lines.push(
                if self.memory.formation_active == Some(true) {
                    "Organizing conversations / 正在后台整理对话"
                } else {
                    "No conversation processing active / 当前未在整理对话"
                }
                .into(),
            );
            if self.memory.maintenance_active == Some(true) {
                lines.push("Consolidating memories / 正在维护已有记忆".into());
            }
        } else {
            lines.push(format!(
                "Memory status: {:?} / 无法读取记忆状态",
                self.memory.state
            ));
            lines.push("Check `anda status`; if stopped, run `anda start`. / 先检查 `anda status`；若未启动，运行 `anda start`。".into());
        }
        lines.push(match self.inbox.state {
            ReadState::Available => format!("Attention inbox: {} visible items. / 记忆待办：当前可见 {} 项。\n/brain inbox", self.inbox.visible_items.unwrap_or(0), self.inbox.visible_items.unwrap_or(0)),
            ReadState::NotConfigured => "Attention inbox is optional and not configured. Ordinary memory needs no extra setup. / 记忆待办未配置；普通记忆无需额外配置。".into(),
            ReadState::Forbidden | ReadState::Unauthorized => "This identity cannot read the attention inbox. / 当前身份无法读取记忆待办。".into(),
            _ => "Attention inbox status unavailable; checked separately from memory. / 暂时无法读取待办状态；它与普通记忆分别检查。".into(),
        });
        lines.push("Connection and activity do not prove that a specific fact was saved. / 连接与处理状态不能证明某条信息已保存。".into());
        lines.push("Try / 试一试：记住：我喜欢简短的发布说明，但要保留风险段落。\nAfter background processing, use /new and ask what you prefer. / 等后台整理后，用 /new 开新对话询问这条偏好。\n/memory help · anda memory guide".into());
        lines.join("\n\n")
    }
}
