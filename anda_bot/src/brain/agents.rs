//! Other local agents' access to this Brain, served over MCP by
//! `engine::memory_mcp`.
//!
//! The owner issues each agent (Claude Code, Codex, …) a bearer token for one
//! grant. Only the token's digest is kept, so a lost token is revoked and
//! replaced, never recovered. A grant reads memory, or reads it and writes
//! notes.
//!
//! A note is a message from the agent as a named external participant, kept
//! in a conversation the owner owns and observed through the same journal as
//! the Bot's own windows. Formation therefore attributes it to the agent's
//! Person, never to the owner or to `$self`, and the owner's Memory views
//! verify its source, correct it and delete it like any other.
//!
//! A grant reads what the Bot reads: recall answers from the whole Space,
//! and entity pages show what the owner's Memory views show.
use super::{
    Client, FormationProvenance, FormationState, FormationSubmission, Journal, SourceMessageRef,
};
use crate::engine::{ConversationsTool, scoped_external_user_name};
use anda_brain::types::{FormationInputRef, InputContext, RecallInputRef};
use anda_core::{BoxError, ContentPart, Message, Principal};
use anda_db::error::DBError;
use anda_engine::memory::{Conversation, ConversationRef, ConversationStatus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

/// Every token starts with this, so a leaked one is recognizable.
pub const TOKEN_PREFIX: &str = "anda_mem_";
/// The longest note one call records.
pub const MAX_NOTE_BYTES: usize = 16 * 1024;
/// The longest recall question, as for the owner's memory search.
pub const MAX_QUERY_BYTES: usize = 8192;
pub const DEFAULT_GRANT_DAYS: u64 = 90;
pub const MAX_GRANT_DAYS: u64 = 3650;
const MAX_TOPIC_CHARS: usize = 200;
const MAX_NAME_CHARS: usize = 64;
const MAX_CLIENT_CHARS: usize = 40;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;
const MAX_ACTIVE_GRANTS: usize = 32;
const DAY_MS: u64 = 24 * 3600 * 1000;
/// How long a revoked or expired grant stays listed.
const RETIRED_RETENTION_MS: u64 = 30 * DAY_MS;
/// How often a grant's last use is saved.
const LAST_USED_RESOLUTION_MS: u64 = 3600 * 1000;
/// Notes kept in one notebook conversation before the next one starts.
const NOTES_PER_CONVERSATION: usize = 200;
/// How long a recall waits for the agent's own notes still being processed.
/// Shorter than the Bot's: an agent asks often and is told what is pending.
const NOTE_BARRIER: Duration = Duration::from_secs(5);
/// Concurrent recalls across every agent; each runs a Brain model pass.
const CONCURRENT_RECALLS: usize = 2;
const REGISTRY_KEY: &str = "agent-grants";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentScope {
    Read,
    Write,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentGrant {
    pub id: String,
    /// The agent's stable handle, such as `claude-code`. Across its tokens, it
    /// names the Person its notes are attributed to and their source.
    pub client: String,
    /// The Person's display name, such as `Claude Code`.
    pub name: String,
    pub scope: AgentScope,
    pub created_at: u64,
    pub expires_at: u64,
    #[serde(default)]
    pub revoked_at: Option<u64>,
    #[serde(default)]
    pub last_used_at: Option<u64>,
}

impl AgentGrant {
    pub fn is_active(&self, now_ms: u64) -> bool {
        self.revoked_at.is_none() && now_ms < self.expires_at
    }

    pub fn can_write(&self) -> bool {
        self.scope == AgentScope::Write
    }

    /// The Person key Formation attributes this agent's notes to.
    pub fn participant(&self) -> String {
        participant(&self.client)
    }

    /// The source this agent's notes and questions name.
    pub fn source(&self) -> String {
        format!("agent:{}", self.client)
    }

    fn retired_at(&self) -> u64 {
        self.revoked_at.unwrap_or(self.expires_at)
    }
}

/// An external agent's participant key, in the form IM senders have, so the
/// apps show its notes as an external participant's and the runtime never
/// takes them for the owner's.
pub fn participant(client: &str) -> String {
    scoped_external_user_name("agent", None, client)
}

/// The display name of a well-known client.
pub fn default_name(client: &str) -> String {
    match client {
        "claude-code" => "Claude Code",
        "claude" | "claude-desktop" => "Claude",
        "codex" => "Codex",
        "chatgpt" => "ChatGPT",
        "cursor" => "Cursor",
        "gemini" | "gemini-cli" => "Gemini CLI",
        "copilot" => "GitHub Copilot",
        "windsurf" => "Windsurf",
        _ => client,
    }
    .to_string()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrantRequest {
    pub client: String,
    #[serde(default)]
    pub name: Option<String>,
    pub scope: AgentScope,
    #[serde(default)]
    pub days: Option<u64>,
}

/// A new grant with its token, which is never shown again.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IssuedGrant {
    pub grant: AgentGrant,
    pub token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GrantList {
    pub schema_version: u32,
    pub items: Vec<AgentGrant>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RevokedGrants {
    pub schema_version: u32,
    pub revoked: Vec<AgentGrant>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct Registry {
    grants: Vec<StoredGrant>,
    /// Each agent's current notebook conversation, by client.
    #[serde(default)]
    notebooks: BTreeMap<String, u64>,
}

#[derive(Clone, Deserialize, Serialize)]
struct StoredGrant {
    grant: AgentGrant,
    token_digest: String,
}

/// The owner's grants, kept in the Brain journal (`bot-brain/v1/agent-grants`).
#[derive(Clone)]
pub struct AgentGrants {
    journal: Journal,
    registry: Arc<tokio::sync::Mutex<Registry>>,
}

impl AgentGrants {
    pub async fn load(journal: Journal) -> Result<Self, BoxError> {
        let registry = journal
            .read::<Registry>(REGISTRY_KEY)
            .await?
            .unwrap_or_default();
        Ok(Self {
            journal,
            registry: Arc::new(tokio::sync::Mutex::new(registry)),
        })
    }

    /// Every grant still listed, newest first.
    pub async fn list(&self) -> GrantList {
        let registry = self.registry.lock().await;
        let mut items: Vec<AgentGrant> = registry
            .grants
            .iter()
            .map(|stored| stored.grant.clone())
            .collect();
        items.sort_by_key(|grant| std::cmp::Reverse(grant.created_at));
        GrantList {
            schema_version: 1,
            items,
        }
    }

    pub async fn create(
        &self,
        request: GrantRequest,
        now_ms: u64,
    ) -> Result<IssuedGrant, BoxError> {
        let client = request.client.trim().to_ascii_lowercase();
        if !valid_client(&client) {
            return Err("invalid_agent".into());
        }
        let name = match request.name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => default_name(&client),
        };
        if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
            return Err("invalid_agent".into());
        }
        let days = request.days.unwrap_or(DEFAULT_GRANT_DAYS);
        if !(1..=MAX_GRANT_DAYS).contains(&days) {
            return Err("invalid_agent".into());
        }
        let token = new_token();
        let grant = AgentGrant {
            id: ic_auth_types::Xid::new().to_string(),
            client,
            name,
            scope: request.scope,
            created_at: now_ms,
            expires_at: now_ms.saturating_add(days * DAY_MS),
            revoked_at: None,
            last_used_at: None,
        };
        let stored = StoredGrant {
            grant: grant.clone(),
            token_digest: token_digest(&token),
        };
        self.update(|registry| {
            registry.grants.retain(|stored| {
                stored.grant.is_active(now_ms)
                    || stored
                        .grant
                        .retired_at()
                        .saturating_add(RETIRED_RETENTION_MS)
                        > now_ms
            });
            let active = registry
                .grants
                .iter()
                .filter(|stored| stored.grant.is_active(now_ms))
                .count();
            if active >= MAX_ACTIVE_GRANTS {
                return Err("agent_capacity".into());
            }
            registry.grants.push(stored);
            Ok(())
        })
        .await?;
        Ok(IssuedGrant { grant, token })
    }

    /// Revokes the grant with id `target`, or every grant of the client
    /// `target`. Its tokens stop working at once.
    pub async fn revoke(&self, target: &str, now_ms: u64) -> Result<RevokedGrants, BoxError> {
        let target = target.trim();
        let revoked = self
            .update(|registry| {
                let mut revoked = Vec::new();
                for stored in &mut registry.grants {
                    let grant = &mut stored.grant;
                    if grant.revoked_at.is_none() && (grant.id == target || grant.client == target)
                    {
                        grant.revoked_at = Some(now_ms);
                        revoked.push(grant.clone());
                    }
                }
                if revoked.is_empty() {
                    return Err("agent_not_found".into());
                }
                Ok(revoked)
            })
            .await?;
        Ok(RevokedGrants {
            schema_version: 1,
            revoked,
        })
    }

    /// The active grant `token` names. Its last use is saved at most hourly.
    pub async fn verify(&self, token: &str, now_ms: u64) -> Option<AgentGrant> {
        if !token.starts_with(TOKEN_PREFIX) {
            return None;
        }
        let digest = token_digest(token);
        let mut registry = self.registry.lock().await;
        let index = registry
            .grants
            .iter()
            .position(|stored| stored.token_digest == digest)?;
        let grant = &registry.grants[index].grant;
        if !grant.is_active(now_ms) {
            return None;
        }
        if grant
            .last_used_at
            .is_none_or(|at| now_ms >= at.saturating_add(LAST_USED_RESOLUTION_MS))
        {
            let mut next = registry.clone();
            next.grants[index].grant.last_used_at = Some(now_ms);
            match self.journal.write(REGISTRY_KEY, &next).await {
                Ok(()) => *registry = next,
                Err(error) => log::warn!("agent grant {}: last use not saved: {error}", grant.id),
            }
        }
        Some(registry.grants[index].grant.clone())
    }

    async fn notebook(&self, client: &str) -> Option<u64> {
        self.registry.lock().await.notebooks.get(client).copied()
    }

    async fn set_notebook(&self, client: &str, conversation: u64) -> Result<(), BoxError> {
        self.update(|registry| {
            registry.notebooks.insert(client.to_string(), conversation);
            Ok(())
        })
        .await
    }

    /// Applies `change` to a copy and keeps it once it is saved.
    async fn update<T>(
        &self,
        change: impl FnOnce(&mut Registry) -> Result<T, BoxError>,
    ) -> Result<T, BoxError> {
        let mut registry = self.registry.lock().await;
        let mut next = registry.clone();
        let value = change(&mut next)?;
        self.journal.write(REGISTRY_KEY, &next).await?;
        *registry = next;
        Ok(value)
    }
}

fn valid_client(client: &str) -> bool {
    let mut chars = client.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && client.len() <= MAX_CLIENT_CHARS
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
}

fn new_token() -> String {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    let mut bytes = [0u8; 32];
    rand::Rng::fill_bytes(&mut rand::rng(), &mut bytes);
    format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

fn token_digest(token: &str) -> String {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD.encode(<sha2::Sha256 as sha2::Digest>::digest(token.as_bytes()))
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteInput {
    pub content: String,
    #[serde(default)]
    pub topic: Option<String>,
    /// The same key replays the first call's note instead of adding another.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NoteReceipt {
    /// `<conversation>/<message index>`.
    pub note_id: String,
    pub state: FormationState,
    /// True when an idempotency key matched an earlier note.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replayed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentRecall {
    pub answer: String,
    /// The agent's own notes still being processed, when there are any.
    pub note: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
struct NoteRef {
    conversation: u64,
    index: usize,
    #[serde(default)]
    topic: Option<String>,
}

/// What agents may do with this Brain under their grants.
pub struct AgentAccess {
    pub grants: AgentGrants,
    client: Client,
    conversations: Arc<ConversationsTool>,
    owner: Principal,
    recalls: tokio::sync::Semaphore,
    /// Participants whose Person already carries their display name.
    named: parking_lot::Mutex<HashSet<(String, String)>>,
}

impl AgentAccess {
    pub fn new(
        grants: AgentGrants,
        client: Client,
        conversations: Arc<ConversationsTool>,
        owner: Principal,
    ) -> Self {
        Self {
            grants,
            client,
            conversations,
            owner,
            recalls: tokio::sync::Semaphore::new(CONCURRENT_RECALLS),
            named: Default::default(),
        }
    }

    pub fn owner(&self) -> Principal {
        self.owner
    }

    /// Asks memory a question for the agent: the owner is who it works for,
    /// the agent who is asking. Waits briefly for its own pending notes.
    pub async fn recall(
        &self,
        grant: &AgentGrant,
        query: &str,
        topic: Option<&str>,
    ) -> Result<AgentRecall, BoxError> {
        let query = query.trim();
        if query.is_empty() {
            return Err("invalid_request".into());
        }
        if query.len() > MAX_QUERY_BYTES {
            return Err("payload_too_large".into());
        }
        let topic = valid_topic(topic)?;
        let _permit = self.recalls.try_acquire().map_err(|_| "capacity")?;
        let owner = self.owner.to_string();
        let notebook = self.grants.notebook(&grant.client).await;
        let barrier = match (self.client.embedded_host(), self.client.journal(), notebook) {
            (Some(host), Some(journal), Some(conversation)) => match host
                .memory_barrier(&journal, &owner, conversation, NOTE_BARRIER)
                .await
            {
                Ok(barrier) => Some((journal, conversation, barrier)),
                Err(error) => {
                    log::warn!("agent recall barrier for {}: {error}", grant.client);
                    None
                }
            },
            _ => None,
        };
        let context = Some(InputContext {
            counterparty: Some(owner.clone()),
            agent: Some(grant.name.clone()),
            source: Some(grant.source()),
            topic: topic.map(str::to_string),
        });
        let output = self
            .client
            .recall(RecallInputRef {
                query,
                context: &context,
                budget: &None,
            })
            .await?;
        if let Some(reason) = output.failed_reason {
            return Err(format!(
                "recall_failed: {}",
                reason.chars().take(512).collect::<String>()
            )
            .into());
        }
        let note = barrier.and_then(|(journal, conversation, barrier)| {
            if !barrier.settled.is_empty() {
                let accounted: Vec<String> = barrier
                    .settled
                    .iter()
                    .map(|progress| progress.receipt_ref.clone())
                    .collect();
                let owner = owner.clone();
                // A spawned write: the answer is ready and must not wait on it.
                tokio::spawn(async move {
                    if let Err(error) = journal
                        .update_memory_session(&owner, &conversation.to_string(), |session| {
                            session.acknowledge_recall(&accounted);
                            Ok(())
                        })
                        .await
                    {
                        log::warn!("agent recall barrier for {conversation}: {error}");
                    }
                });
            }
            barrier.note()
        });
        Ok(AgentRecall {
            answer: output.content,
            note,
        })
    }

    /// Records a note from the agent and submits it to Formation.
    pub async fn remember(
        &self,
        grant: &AgentGrant,
        input: NoteInput,
    ) -> Result<NoteReceipt, BoxError> {
        if !grant.can_write() {
            return Err("forbidden".into());
        }
        let content = input.content.trim();
        if content.is_empty() {
            return Err("invalid_request".into());
        }
        if content.len() > MAX_NOTE_BYTES {
            return Err("payload_too_large".into());
        }
        let topic = valid_topic(input.topic.as_deref())?;
        let key = match input.idempotency_key.as_deref() {
            Some(key) if key.is_empty() || key.len() > MAX_IDEMPOTENCY_KEY_BYTES => {
                return Err("invalid_request".into());
            }
            Some(key) => Some(format!(
                "agent-notes/{}/{}",
                grant.client,
                &anda_cognitive_nexus::content_digest(&json!(key))?[7..39]
            )),
            None => None,
        };
        let journal = self.client.journal().ok_or("unsupported_capability")?;
        // One writer per agent: notes keep their order in the notebook, and
        // an idempotency key is checked and claimed atomically.
        let lock = journal.formation_lock(&format!("agent-notes/{}", grant.client));
        let _guard = lock.lock().await;

        if let Some(key) = &key
            && let Some(note) = journal.read::<NoteRef>(key).await?
        {
            let conversation = self.conversation(note.conversation).await?;
            let value = conversation
                .and_then(|c| c.messages.get(note.index).cloned())
                .ok_or("note_not_found")?;
            return self.submit(&journal, grant, &note, value, true).await;
        }

        self.ensure_participant(grant).await?;
        let now_ms = anda_engine::unix_ms();
        let mut conversation = self.notebook(grant, now_ms).await?;
        let message = Message {
            role: "user".into(),
            name: Some(grant.participant()),
            content: vec![ContentPart::Text {
                text: content.to_string(),
            }],
            timestamp: Some(now_ms),
            ..Default::default()
        };
        let value = serde_json::to_value(&message)?;
        let note = NoteRef {
            conversation: conversation._id,
            index: conversation.messages.len(),
            topic: topic.map(str::to_string),
        };
        conversation.messages.push(value.clone());
        conversation.updated_at = now_ms;
        self.conversations
            .conversations
            .update_conversation(conversation._id, conversation.to_changes()?)
            .await?;
        self.conversations.events.changed(&self.owner.to_string());
        if let Some(key) = &key {
            journal.write(key, &note).await?;
        }
        self.submit(&journal, grant, &note, value, false).await
    }

    /// Where an agent's note stands. Only its own notes are visible to it.
    pub async fn note_status(
        &self,
        grant: &AgentGrant,
        note_id: &str,
    ) -> Result<NoteReceipt, BoxError> {
        let (conversation, index) = note_id
            .split_once('/')
            .and_then(|(c, i)| Some((c.parse::<u64>().ok()?, i.parse::<usize>().ok()?)))
            .ok_or("invalid_request")?;
        let own = self
            .conversation(conversation)
            .await?
            .is_some_and(|c| conversation_source(&c) == Some(grant.source()));
        if !own {
            return Err("note_not_found".into());
        }
        let journal = self.client.journal().ok_or("unsupported_capability")?;
        let mut row = journal
            .read::<FormationSubmission>(&format!("formation/{conversation}/{index}"))
            .await?
            .ok_or("note_not_found")?;
        if let Err(error) = journal.refresh_formation(&self.client, &mut row).await {
            log::debug!("agent note {note_id}: progress not refreshed: {error}");
        }
        Ok(NoteReceipt {
            note_id: note_id.to_string(),
            state: row.state,
            replayed: false,
            error: row.error,
        })
    }

    /// Submits a note's one-message window. The journal observes it under a
    /// key naming the window, so a resubmission replays its receipt.
    async fn submit(
        &self,
        journal: &Journal,
        grant: &AgentGrant,
        note: &NoteRef,
        value: serde_json::Value,
        replayed: bool,
    ) -> Result<NoteReceipt, BoxError> {
        let owner = self.owner.to_string();
        let message: Message = serde_json::from_value(value.clone())?;
        let source = grant.source();
        let submission = FormationSubmission {
            bot_conversation: note.conversation,
            window_start: note.index,
            window_end: note.index + 1,
            submitted_at: anda_engine::unix_ms(),
            observed_at: None,
            brain_conversation: None,
            state: FormationState::Pending,
            error: None,
            provenance: Some(FormationProvenance {
                policy_revision: None,
                version: 1,
                caller: owner.clone(),
                session: None,
                source_identity: Some(anda_brain::product::SourceIdentity {
                    key: super::product::source_identity(&owner, note.conversation, None).key,
                    parents: vec![format!("anda-bot/agent/{owner}/{}", grant.client)],
                }),
                source: source.clone(),
                reply_target: None,
                thread: None,
                external_user: true,
                counterparty: Some(grant.participant()),
                source_messages: vec![SourceMessageRef {
                    conversation: note.conversation.to_string(),
                    index: note.index.to_string(),
                    role: message.role.clone(),
                    content_digest: anda_cognitive_nexus::content_digest(&value)?,
                    submitted_digest: None,
                }],
                input_digest: None,
            }),
            updated_at: None,
            failure_stage: None,
            receipt_ref: None,
            attempt: 0,
        };
        let context = Some(InputContext {
            counterparty: Some(grant.participant()),
            agent: Some(grant.name.clone()),
            source: Some(source),
            topic: note.topic.clone(),
        });
        let timestamp = message.timestamp.and_then(anda_engine::rfc3339_datetime);
        let note_id = format!("{}/{}", note.conversation, note.index);
        let submitted = self
            .client
            .submit_formation_window(
                submission,
                FormationInputRef {
                    messages: std::slice::from_ref(&message),
                    context: &context,
                    timestamp: &timestamp,
                },
            )
            .await;
        let row = match submitted {
            Ok(row) => row,
            // The journal keeps the window with its failure; the same
            // idempotency key resubmits it.
            Err(error) => journal
                .read::<FormationSubmission>(&format!("formation/{note_id}"))
                .await?
                .ok_or(error)?,
        };
        Ok(NoteReceipt {
            note_id,
            state: row.state,
            replayed,
            error: row.error,
        })
    }

    /// Gives the agent's Person its display name, once per process and name.
    async fn ensure_participant(&self, grant: &AgentGrant) -> Result<(), BoxError> {
        let named = (grant.client.clone(), grant.name.clone());
        if self.named.lock().contains(&named) {
            return Ok(());
        }
        self.client
            .user_info(grant.participant(), Some(grant.name.clone()))
            .await?;
        self.named.lock().insert(named);
        Ok(())
    }

    /// The agent's current notebook, or a new one when it is full or gone.
    async fn notebook(&self, grant: &AgentGrant, now_ms: u64) -> Result<Conversation, BoxError> {
        if let Some(id) = self.grants.notebook(&grant.client).await
            && let Some(conversation) = self.conversation(id).await?
            && conversation_source(&conversation) == Some(grant.source())
            && conversation.messages.len() < NOTES_PER_CONVERSATION
        {
            return Ok(conversation);
        }
        let extra = serde_json::Map::from_iter([(
            crate::util::request_meta::keys::SOURCE.to_string(),
            json!(grant.source()),
        )]);
        let mut conversation = Conversation {
            user: self.owner,
            status: ConversationStatus::Completed,
            extra: Some(extra.into()),
            period: now_ms / 3600 / 1000,
            created_at: now_ms,
            updated_at: now_ms,
            ..Default::default()
        };
        conversation._id = self
            .conversations
            .conversations
            .add_conversation(ConversationRef::from(&conversation))
            .await?;
        self.grants
            .set_notebook(&grant.client, conversation._id)
            .await?;
        Ok(conversation)
    }

    /// The owner's conversation `id`, or `None`.
    async fn conversation(&self, id: u64) -> Result<Option<Conversation>, BoxError> {
        match self.conversations.conversations.get_conversation(id).await {
            Ok(conversation) if conversation.user == self.owner => Ok(Some(conversation)),
            Ok(_) | Err(DBError::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

fn conversation_source(conversation: &Conversation) -> Option<String> {
    conversation
        .extra
        .as_ref()?
        .get(crate::util::request_meta::keys::SOURCE)?
        .as_str()
        .map(str::to_string)
}

fn valid_topic(topic: Option<&str>) -> Result<Option<&str>, BoxError> {
    let topic = topic.map(str::trim).filter(|topic| !topic.is_empty());
    if topic.is_some_and(|topic| {
        topic.chars().count() > MAX_TOPIC_CHARS || topic.chars().any(char::is_control)
    }) {
        return Err("invalid_request".into());
    }
    Ok(topic)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> Journal {
        Journal::new(Arc::new(object_store::memory::InMemory::new()))
    }

    fn request(client: &str, scope: AgentScope) -> GrantRequest {
        GrantRequest {
            client: client.into(),
            name: None,
            scope,
            days: Some(30),
        }
    }

    #[tokio::test]
    async fn grants_keep_only_digests_and_verify_until_revoked_or_expired() {
        let journal = journal();
        let grants = AgentGrants::load(journal.clone()).await.unwrap();
        let now = 1_000_000;
        let issued = grants
            .create(request(" Claude-Code ", AgentScope::Write), now)
            .await
            .unwrap();
        assert!(issued.token.starts_with(TOKEN_PREFIX));
        assert_eq!(issued.grant.client, "claude-code");
        assert_eq!(issued.grant.name, "Claude Code");
        assert_eq!(issued.grant.expires_at, now + 30 * DAY_MS);
        assert_eq!(
            issued.grant.participant(),
            "$external_user:\"agent/claude-code\""
        );
        assert_eq!(issued.grant.source(), "agent:claude-code");

        // The journal holds the digest, never the token.
        let stored = serde_json::to_string(
            &journal
                .read::<serde_json::Value>(REGISTRY_KEY)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(!stored.contains(&issued.token));
        assert!(stored.contains(&token_digest(&issued.token)));

        let verified = grants.verify(&issued.token, now + 1).await.unwrap();
        assert_eq!(verified.id, issued.grant.id);
        assert_eq!(verified.last_used_at, Some(now + 1));
        assert!(grants.verify("anda_mem_forged", now + 1).await.is_none());
        assert!(grants.verify(&issued.token[1..], now + 1).await.is_none());
        assert!(
            grants
                .verify(&issued.token, issued.grant.expires_at)
                .await
                .is_none()
        );

        // A restart reads the same grants back.
        let reloaded = AgentGrants::load(journal.clone()).await.unwrap();
        assert!(reloaded.verify(&issued.token, now + 2).await.is_some());

        let read = reloaded
            .create(request("codex", AgentScope::Read), now)
            .await
            .unwrap();
        let revoked = reloaded.revoke("claude-code", now + 3).await.unwrap();
        assert_eq!(revoked.revoked.len(), 1);
        assert!(reloaded.verify(&issued.token, now + 4).await.is_none());
        assert!(reloaded.verify(&read.token, now + 4).await.is_some());
        assert_eq!(
            reloaded
                .revoke("claude-code", now + 5)
                .await
                .unwrap_err()
                .to_string(),
            "agent_not_found"
        );
        let revoked = reloaded.revoke(&read.grant.id, now + 5).await.unwrap();
        assert_eq!(revoked.revoked[0].client, "codex");
        assert_eq!(reloaded.list().await.items.len(), 2);

        // Retired grants are pruned a month after they stop working.
        let later = now + 5 + RETIRED_RETENTION_MS + 1;
        reloaded
            .create(request("cursor", AgentScope::Read), later)
            .await
            .unwrap();
        let listed = reloaded.list().await.items;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Cursor");
    }

    #[tokio::test]
    async fn grant_requests_are_validated_and_capped() {
        let grants = AgentGrants::load(journal()).await.unwrap();
        for bad in [
            GrantRequest {
                client: "".into(),
                ..request("x", AgentScope::Read)
            },
            GrantRequest {
                client: "-claude".into(),
                ..request("x", AgentScope::Read)
            },
            GrantRequest {
                client: "claude code".into(),
                ..request("x", AgentScope::Read)
            },
            GrantRequest {
                client: "a".repeat(MAX_CLIENT_CHARS + 1),
                ..request("x", AgentScope::Read)
            },
            GrantRequest {
                name: Some("bad\nname".into()),
                ..request("x", AgentScope::Read)
            },
            GrantRequest {
                days: Some(0),
                ..request("x", AgentScope::Read)
            },
            GrantRequest {
                days: Some(MAX_GRANT_DAYS + 1),
                ..request("x", AgentScope::Read)
            },
        ] {
            assert_eq!(
                grants.create(bad, 1).await.unwrap_err().to_string(),
                "invalid_agent"
            );
        }
        let named = grants
            .create(
                GrantRequest {
                    name: Some("  My Agent ".into()),
                    days: None,
                    ..request("my-agent.v2", AgentScope::Read)
                },
                1,
            )
            .await
            .unwrap();
        assert_eq!(named.grant.name, "My Agent");
        assert_eq!(named.grant.expires_at, 1 + DEFAULT_GRANT_DAYS * DAY_MS);
        for i in 1..MAX_ACTIVE_GRANTS {
            grants
                .create(request(&format!("agent-{i}"), AgentScope::Read), 1)
                .await
                .unwrap();
        }
        assert_eq!(
            grants
                .create(request("one-more", AgentScope::Read), 1)
                .await
                .unwrap_err()
                .to_string(),
            "agent_capacity"
        );
    }
}
