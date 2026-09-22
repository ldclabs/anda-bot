//! Native claims mapped to verified Bot source records; never inferred from text.
use super::{
    Host,
    activity::{ActivityStore, MemorySource},
};
use anda_core::{BoxError, Principal};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordQuery {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemoryRecordView {
    pub id: String,
    pub revision: String,
    pub text: String,
    #[serde(default = "other_kind")]
    pub kind: String,
    #[serde(default)]
    pub scope: serde_json::Value,
    pub effective_at: Option<String>,
    pub subject_label: String,
    pub predicate_label: String,
    pub object_label: String,
    pub about_owner: bool,
    pub stance: String,
    pub state: String,
    pub updated_at: Option<String>,
    pub sources: Vec<MemorySource>,
    pub sources_complete: bool,
    pub allowed_actions: Vec<String>,
}

fn other_kind() -> String {
    "other".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecordPage {
    pub schema_version: u32,
    pub items: Vec<MemoryRecordView>,
    pub complete: bool,
    pub partial_reason: Option<String>,
}

pub async fn list(
    host: &Host,
    activity: &ActivityStore,
    caller: Principal,
    query: RecordQuery,
) -> Result<(RecordPage, Option<String>), BoxError> {
    let limit = query.limit.unwrap_or(20);
    if !(1..=50).contains(&limit) {
        return Err("invalid_request".into());
    }
    let cursor = query
        .cursor
        .map(|c| c.parse::<u64>())
        .transpose()
        .map_err(|_| "invalid_cursor")?;
    let native = host
        .state
        .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
        .await?
        .product_records(cursor, limit)
        .await?;
    let mut items = Vec::new();
    let mut source_gap = false;
    let mut response_bytes = 0;
    for record in native.records {
        match project(record, activity, caller).await? {
            Some(record) => {
                source_gap |= !record.sources_complete;
                let size = serde_json::to_vec(&record)?.len();
                if size > 131_072 || response_bytes + size > 262_144 {
                    source_gap = true;
                    continue;
                }
                response_bytes += size;
                items.push(record)
            }
            None => source_gap = true,
        }
    }
    Ok((
        RecordPage {
            schema_version: 1,
            items,
            complete: native.complete && !source_gap,
            partial_reason: source_gap.then(|| "source_provenance_incomplete".into()),
        },
        native.next_cursor.map(|v| v.to_string()),
    ))
}

pub async fn get(
    host: &Host,
    activity: &ActivityStore,
    caller: Principal,
    id: &str,
) -> Result<MemoryRecordView, BoxError> {
    let id = id
        .parse::<anda_cognitive_nexus::ElementId>()
        .map_err(|_| "invalid_request")?;
    let record = host
        .state
        .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
        .await?
        .product_record(&id.to_string())
        .await
        .map_err(|_| "not_found")?;
    let record = project(record, activity, caller)
        .await?
        .ok_or("not_found")?;
    if serde_json::to_vec(&record)?.len() > 131_072 {
        return Err("unsupported_scope".into());
    }
    Ok(record)
}

async fn project(
    record: anda_brain::product::MemoryRecord,
    activity: &ActivityStore,
    caller: Principal,
) -> Result<Option<MemoryRecordView>, BoxError> {
    let mut sources = Vec::new();
    for source in record.sources.iter().take(32) {
        if let Some(source) = activity.resolve_source(caller, source).await? {
            sources.push(source)
        }
    }
    let complete = record.sources_complete && sources.len() == record.sources.len();
    // Legacy claims can only appear when their semantic actor is this owner.
    // A partial mixture of attributed sources must never expose a compound
    // conclusion just because one source belongs to the requesting caller.
    if !complete
        && (record.actor_key.as_deref() != Some(&caller.to_string()) || !sources.is_empty())
    {
        return Ok(None);
    }
    let about_owner = record.actor_key.as_deref() == Some(&caller.to_string())
        && record.subject.get("id").and_then(serde_json::Value::as_str)
            == record.actor_id.as_deref();
    let predicate_label = record
        .predicate
        .rsplit('/')
        .next()
        .unwrap_or(&record.predicate)
        .to_string();
    let mut allowed_actions = Vec::new();
    if complete {
        allowed_actions.push("delete".to_string());
        if record.storage_state == "active" {
            allowed_actions.push("suppress".into());
        }
        if record.actor_key.as_deref() == Some(&caller.to_string())
            && record.stance == "support"
            && record.status == "active"
            && record.storage_state == "active"
            && record.object.get("id").is_some()
        {
            allowed_actions.push("correct".into());
        }
    }
    Ok(Some(MemoryRecordView {
        kind: if predicate_label == "prefers" {
            "preference"
        } else {
            "other"
        }
        .into(),
        scope: serde_json::json!({"subject":record.subject,"valid_from":record.valid_from,"valid_until":record.valid_until}),
        effective_at: record.valid_from,
        id: record.id,
        revision: record.revision.to_string(),
        text: record.text,
        subject_label: record.subject_label,
        predicate_label,
        object_label: record.object_label,
        about_owner,
        stance: record.stance,
        state: if record.storage_state == "active" {
            record.status
        } else {
            record.storage_state
        },
        updated_at: (!record.updated_at.is_empty()).then_some(record.updated_at),
        sources,
        sources_complete: complete,
        allowed_actions,
    }))
}
