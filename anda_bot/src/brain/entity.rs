//! Entity pages: one Concept's claims, newest first, a bounded page at a time.
//!
//! A claim is shown only when it passes the record list's source check, so an
//! entity page never names, counts or links what that check hides. Belief
//! status comes from the standard recall policy, never from mere Proposition
//! existence.
use super::{
    Host,
    activity::{ActivityStore, SourceResolver},
    catalog::{self, MemoryRecordView},
};
use anda_brain::space::Space;
use anda_core::{BoxError, Principal};
use anda_kip::{OperationStatus, Request, Response, TopLevelStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// Rows read from the Nexus per query page.
const KIP_PAGE: usize = 50;
/// Claims inspected per request, visible or not.
const SCAN_BUDGET: usize = 200;
/// Claims inspected per search hit before it is treated as not visible.
const SEARCH_SCAN_BUDGET: usize = 40;
const SEARCH_HITS: usize = 20;
const SEARCH_RESULTS: usize = 10;
const RESPONSE_BUDGET: usize = 262_144;
/// Entity pages quote sources briefly; the record view has the full quote.
const QUOTE_CHARS: usize = 280;

const CLAIMS: &str = r#"FIND(?a.id, ?subject.id) WHERE {
  ?e CONCEPT {id: :id}
  ?p (?e, ?predicate, ?object)
  UNION {
    ?e CONCEPT {id: :id}
    ?p (?subject, ?predicate, ?e)
  }
  ?a ASSERTION {proposition: ?p}
}
ORDER BY ?a._system.created_at DESC, ?a.id DESC
LIMIT :limit"#;
const BELIEF: &str = r#"FIND(?b.status, ?b.explanation.excluded) WHERE { ?b BELIEF (id: :id) }
WITH EPISTEMIC {purpose: "answer_user", risk: "low", policy: "kip:memory-default"}"#;
const CONCEPT: &str =
    r#"FIND(?e.id, ?e.name, ?e.schema_ref, ?e.key) WHERE { ?e CONCEPT {id: :id} } LIMIT 1"#;
const OWNER: &str = r#"FIND(?e.id, ?e.name, ?e.schema_ref, ?e.key) WHERE { ?e {type: "Person", key: :key} } LIMIT 1"#;
const SEARCH: &str = "SEARCH CONCEPT :query LIMIT :limit";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityQuery {
    /// A Concept id. Omitted, the page is about the caller.
    pub id: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntitySearchQuery {
    pub query: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EntityView {
    pub id: String,
    pub name: String,
    /// The local name of the Concept type, such as `Person`.
    #[serde(rename = "type")]
    pub type_name: String,
    pub about_owner: bool,
}

/// The other end of a claim. A literal value has no id.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityLink {
    pub id: Option<String>,
    pub label: String,
}

/// The Proposition's projection under `kip:memory-default`. `excluded_reason`
/// says why this claim's own Assertion did not count, e.g.
/// `outside_valid_time` once a newer value took over.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BeliefView {
    pub status: String,
    pub excluded_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityClaim {
    /// `outgoing` when the entity is the subject, `incoming` when the object.
    pub direction: String,
    pub other: EntityLink,
    pub belief: Option<BeliefView>,
    pub record: MemoryRecordView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntityPage {
    pub schema_version: u32,
    pub entity: EntityView,
    pub items: Vec<EntityClaim>,
    pub complete: bool,
    pub partial_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EntitySearchPage {
    pub schema_version: u32,
    pub items: Vec<EntityView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EntityCursor {
    entity: String,
    /// The Nexus cursor of the query page to resume in; none for the first.
    kip: Option<String>,
    /// Rows of that query page already consumed.
    skip: usize,
}

#[derive(Default)]
struct Scan {
    claims: Vec<(anda_brain::product::MemoryRecord, MemoryRecordView, bool)>,
    next: Option<EntityCursor>,
    hidden: bool,
    scan_limited: bool,
    size_limited: bool,
}

pub async fn page(
    host: &Host,
    activity: &ActivityStore,
    caller: Principal,
    query: EntityQuery,
) -> Result<(EntityPage, Option<String>), BoxError> {
    let limit = query.limit.unwrap_or(20);
    if !(1..=50).contains(&limit) {
        return Err("invalid_request".into());
    }
    let space = host
        .state
        .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
        .await?;
    let entity = match &query.id {
        Some(id) => concept(&space, id, caller).await?,
        None => owner(&space, caller).await?,
    }
    .ok_or("entity_not_found")?;
    let start = match &query.cursor {
        Some(text) => {
            let cursor: EntityCursor = serde_json::from_str(text).map_err(|_| "invalid_cursor")?;
            if cursor.entity != entity.id {
                return Err("invalid_cursor".into());
            }
            cursor
        }
        None => EntityCursor {
            entity: entity.id.clone(),
            kip: None,
            skip: 0,
        },
    };
    let mut resolver = activity.source_resolver(caller);
    let scan = scan(&space, &mut resolver, start, limit, SCAN_BUDGET).await?;
    // Nothing the owner may see names this entity: say so without its name.
    if query.cursor.is_none() && scan.claims.is_empty() && !entity.about_owner {
        return Err("entity_not_found".into());
    }
    let propositions: Vec<String> = scan
        .claims
        .iter()
        .map(|(native, _, _)| native.proposition_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let beliefs = beliefs(&space, &propositions).await;
    let items = scan
        .claims
        .into_iter()
        .map(|(native, record, outgoing)| {
            let endpoint = if outgoing {
                &native.object
            } else {
                &native.subject
            };
            let belief = beliefs
                .get(&native.proposition_id)
                .map(|(status, excluded)| BeliefView {
                    status: status.clone(),
                    excluded_reason: excluded.get(&native.id).cloned(),
                });
            EntityClaim {
                direction: if outgoing { "outgoing" } else { "incoming" }.into(),
                other: EntityLink {
                    id: endpoint
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    label: if outgoing {
                        record.object_label.clone()
                    } else {
                        record.subject_label.clone()
                    },
                },
                belief,
                record,
            }
        })
        .collect();
    let next_cursor = scan.next.map(|c| serde_json::to_string(&c)).transpose()?;
    Ok((
        EntityPage {
            schema_version: 1,
            entity,
            items,
            complete: !scan.hidden && !scan.scan_limited && !scan.size_limited,
            partial_reason: if scan.size_limited {
                Some("response_size_limit".into())
            } else if scan.scan_limited {
                Some("scan_limit".into())
            } else {
                scan.hidden.then(|| "source_provenance_incomplete".into())
            },
        },
        next_cursor,
    ))
}

/// Keyword search over Concept names that returns only entities with at least
/// one claim the owner may see, and the owner's own entity.
pub async fn search(
    host: &Host,
    activity: &ActivityStore,
    caller: Principal,
    query: EntitySearchQuery,
) -> Result<EntitySearchPage, BoxError> {
    let text = query.query.trim();
    if text.is_empty() {
        return Err("invalid_request".into());
    }
    if text.len() > 8192 {
        return Err("payload_too_large".into());
    }
    let space = host
        .state
        .load_space(crate::config::ANDA_BOT_SPACE_ID, true)
        .await?;
    let response = kip(
        &space,
        json!({"kip":"2.0","operations":[{"command":SEARCH,"parameters":{"query":text,"limit":SEARCH_HITS}}]}),
    )
    .await?;
    let hits = succeeded(&response, 0)?
        .get("hits")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut resolver = activity.source_resolver(caller);
    let mut items = Vec::new();
    for hit in hits {
        let Some(entity) = hit.get("element").and_then(|e| view(e, caller)) else {
            continue;
        };
        if items.iter().any(|item: &EntityView| item.id == entity.id) {
            continue;
        }
        let visible = entity.about_owner
            || !scan(
                &space,
                &mut resolver,
                EntityCursor {
                    entity: entity.id.clone(),
                    kip: None,
                    skip: 0,
                },
                1,
                SEARCH_SCAN_BUDGET,
            )
            .await?
            .claims
            .is_empty();
        if visible {
            items.push(entity);
            if items.len() == SEARCH_RESULTS {
                break;
            }
        }
    }
    Ok(EntitySearchPage {
        schema_version: 1,
        items,
    })
}

/// Collects up to `limit` visible claims from `start`, inspecting at most
/// `budget` rows, and returns where the next page resumes.
async fn scan(
    space: &Space,
    resolver: &mut SourceResolver<'_>,
    start: EntityCursor,
    limit: usize,
    budget: usize,
) -> Result<Scan, BoxError> {
    let mut result = Scan::default();
    let mut position = start;
    let mut seen = HashSet::new();
    let mut scanned = 0;
    let mut bytes = 1024; // Envelope, entity header, reason and cursor.
    loop {
        let mut request = json!({"kip":"2.0","operations":[{"command":CLAIMS,"parameters":{"id":position.entity,"limit":KIP_PAGE}}]});
        if let Some(cursor) = &position.kip {
            request["operations"][0]["command"] = format!("{CLAIMS} CURSOR :cursor").into();
            request["operations"][0]["parameters"]["cursor"] = cursor.clone().into();
        }
        let response = kip(space, request).await?;
        let rows = succeeded(&response, 0)
            .map_err(|error| {
                if position.kip.is_some() {
                    "invalid_cursor".into()
                } else {
                    error
                }
            })?
            .as_array()
            .cloned()
            .unwrap_or_default();
        let kip_next = response.results[0].next_cursor.clone();
        if position.skip > rows.len() {
            return Err("invalid_cursor".into());
        }
        // Resume after `index`: in the same query page, or at the next one.
        let after = |position: &EntityCursor, index: usize| {
            if index + 1 < rows.len() {
                Some(EntityCursor {
                    skip: index + 1,
                    ..position.clone()
                })
            } else {
                kip_next.clone().map(|kip| EntityCursor {
                    entity: position.entity.clone(),
                    kip: Some(kip),
                    skip: 0,
                })
            }
        };
        for (index, row) in rows.iter().enumerate().skip(position.skip) {
            if scanned == budget {
                result.scan_limited = true;
                result.next = Some(EntityCursor {
                    skip: index,
                    ..position.clone()
                });
                return Ok(result);
            }
            scanned += 1;
            let Some(id) = row.get(0).and_then(Value::as_str) else {
                result.hidden = true;
                continue;
            };
            // The incoming branch binds the subject; the outgoing leaves it null.
            let outgoing = row.get(1).is_none_or(Value::is_null);
            if !seen.insert((id.to_string(), outgoing)) {
                continue;
            }
            let Ok(native) = space.product_record(id).await else {
                result.hidden = true;
                continue;
            };
            let Some(mut record) = catalog::project(native.clone(), resolver).await? else {
                result.hidden = true;
                continue;
            };
            for source in &mut record.sources {
                if let Some(text) = &mut source.text
                    && let Some((offset, _)) = text.char_indices().nth(QUOTE_CHARS)
                {
                    text.truncate(offset);
                    source.text_truncated = true;
                }
            }
            let size = catalog::compact_display(&mut record)?;
            if bytes + size + 1 > RESPONSE_BUDGET {
                result.size_limited = true;
                result.next = Some(EntityCursor {
                    skip: index,
                    ..position.clone()
                });
                return Ok(result);
            }
            bytes += size + 1;
            result.claims.push((native, record, outgoing));
            if result.claims.len() == limit {
                result.next = after(&position, index);
                return Ok(result);
            }
        }
        match kip_next {
            Some(kip) => {
                position = EntityCursor {
                    entity: position.entity,
                    kip: Some(kip),
                    skip: 0,
                }
            }
            None => return Ok(result),
        }
    }
}

/// Belief status per Proposition. A failed projection leaves claims without
/// one rather than failing the page.
async fn beliefs(
    space: &Space,
    propositions: &[String],
) -> HashMap<String, (String, HashMap<String, String>)> {
    if propositions.is_empty() {
        return HashMap::new();
    }
    let operations: Vec<Value> = propositions
        .iter()
        .map(|id| json!({"command":BELIEF,"parameters":{"id":id}}))
        .collect();
    let Ok(response) = kip(
        space,
        json!({"kip":"2.0","operations":operations,"execution":{"mode":"independent"}}),
    )
    .await
    else {
        return HashMap::new();
    };
    let mut beliefs = HashMap::new();
    for (index, id) in propositions.iter().enumerate() {
        let Ok(rows) = succeeded(&response, index) else {
            continue;
        };
        let Some(row) = rows.get(0) else { continue };
        let Some(status) = row.get(0).and_then(Value::as_str) else {
            continue;
        };
        let excluded = row
            .get(1)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some((
                            item.get("assertion_id")?.as_str()?.to_string(),
                            item.get("reason")?.as_str()?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        beliefs.insert(id.clone(), (status.to_string(), excluded));
    }
    beliefs
}

async fn concept(
    space: &Space,
    id: &str,
    caller: Principal,
) -> Result<Option<EntityView>, BoxError> {
    if !id
        .strip_prefix("C-")
        .is_some_and(|seq| !seq.is_empty() && seq.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("invalid_request".into());
    }
    header(space, CONCEPT, json!({"id":id}), caller).await
}

async fn owner(space: &Space, caller: Principal) -> Result<Option<EntityView>, BoxError> {
    header(space, OWNER, json!({"key":caller.to_string()}), caller).await
}

async fn header(
    space: &Space,
    command: &str,
    parameters: Value,
    caller: Principal,
) -> Result<Option<EntityView>, BoxError> {
    let response = kip(
        space,
        json!({"kip":"2.0","operations":[{"command":command,"parameters":parameters}]}),
    )
    .await?;
    let row = succeeded(&response, 0)?
        .as_array()
        .and_then(|rows| rows.first())
        .cloned();
    Ok(row.and_then(|row| {
        view(
            &json!({"id":row.get(0)?,"name":row.get(1)?,"schema_ref":row.get(2)?,"key":row.get(3)}),
            caller,
        )
    }))
}

fn view(element: &Value, caller: Principal) -> Option<EntityView> {
    let id = element.get("id")?.as_str()?.to_string();
    let schema_ref = element.get("schema_ref")?.as_str()?;
    let type_name = schema_ref
        .rsplit('/')
        .next()
        .unwrap_or(schema_ref)
        .to_string();
    Some(EntityView {
        name: element
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_string(),
        about_owner: type_name == "Person"
            && element.get("key").and_then(Value::as_str) == Some(&caller.to_string()),
        type_name,
        id,
    })
}

async fn kip(space: &Space, request: Value) -> Result<Response, BoxError> {
    let request: Request = serde_json::from_value(request)?;
    space.execute_kip_readonly(request).await
}

fn succeeded(response: &Response, index: usize) -> Result<&Value, BoxError> {
    let result = response.results.get(index).ok_or("kip_failed")?;
    if !matches!(
        response.status,
        TopLevelStatus::Succeeded | TopLevelStatus::Partial
    ) || result.status != OperationStatus::Succeeded
    {
        return Err("kip_failed".into());
    }
    result.result.as_ref().ok_or_else(|| "kip_failed".into())
}
