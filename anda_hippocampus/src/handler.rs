use anda_core::Principal;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::header,
    response::IntoResponse,
};
use serde_json::json;
use std::str::FromStr;

use crate::payload::{Accept, AppError, AppResponse, BearerToken, RpcResponse};
use crate::space::AppState;
use crate::types::*;

const SKILL_MARKDOWN: &[u8] = include_bytes!("../SKILL.md");

pub async fn get_information(State(app): State<AppState>) -> impl IntoResponse {
    let info = json!({
        "name": app.app_name,
        "version": app.app_version,
        "sharding": app.sharding,
         "description": "Hippocampus is a long-term memory system for LLM agents, providing persistent storage and retrieval of knowledge across interactions. It enables agents to remember facts, preferences, relationships, past events, and any other information that can be useful for answering questions and making decisions. Hippocampus organizes memories in a structured way, allowing efficient search and recall based on natural language queries. By using Hippocampus, agents can maintain context and continuity over time, improving their ability to assist users effectively.",
    });

    Json(info)
}

pub async fn get_skill(State(_app): State<AppState>) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/markdown")], SKILL_MARKDOWN).into_response()
}

/// POST /admin/create_space
pub async fn create_space(
    State(app): State<AppState>,
    Accept(ct): Accept,
    BearerToken(token): BearerToken,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let token = app
        .check_admin(&token, "*", "write")
        .map_err(|_| AppError::unauthorized())?;

    let input: CreateSpaceInput = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let sid = SpaceId::from_str(&input.space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let rt = app
        .create_space(token.user, input.user, sid.id)
        .await
        .map_err(AppError::bad_request)?;
    Ok(AppResponse::new(RpcResponse::success(rt), ct))
}

/// GET /v1/{space_id}/status
pub async fn get_status(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct): Accept,
    BearerToken(token): BearerToken,
) -> Result<impl IntoResponse, AppError> {
    let sid = SpaceId::from_str(&space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    app.check_auth(&token, &sid.id, "read")
        .map_err(|_| AppError::unauthorized())?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;
    let rt = space.get_status().await;
    Ok(AppResponse::new(RpcResponse::success(rt), ct))
}

/// POST /v1/{space_id}/formation
pub async fn post_formation(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct): Accept,
    BearerToken(token): BearerToken,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let sid = SpaceId::from_str(&space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let _ = app
        .check_auth(&token, &sid.id, "write")
        .map_err(|_| AppError::unauthorized())?;
    let input: FormationInput = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;
    // 使用固定的 caller 进行 ingestions 和 queries
    let rt = space
        .ingest(Principal::management_canister(), input)
        .await
        .map_err(AppError::bad_request)?;
    Ok(AppResponse::new(RpcResponse::success(rt), ct))
}

/// POST /v1/{space_id}/recall
pub async fn post_recall(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct): Accept,
    BearerToken(token): BearerToken,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let sid = SpaceId::from_str(&space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let _ = app
        .check_auth(&token, &sid.id, "read")
        .map_err(|_| AppError::unauthorized())?;
    let input: RecallInput = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;
    // 使用固定的 caller 进行 ingestions 和 queries
    let rt = space
        .query(Principal::management_canister(), input)
        .await
        .map_err(AppError::bad_request)?;
    Ok(AppResponse::new(RpcResponse::success(rt), ct))
}

/// POST /v1/{space_id}/maintenance
pub async fn post_maintenance(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct): Accept,
    BearerToken(token): BearerToken,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let sid = SpaceId::from_str(&space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let _ = app
        .check_auth(&token, &sid.id, "write")
        .map_err(|_| AppError::unauthorized())?;
    let input: MaintenanceInput = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;
    let rt = space
        .maintenance(Principal::management_canister(), input)
        .await
        .map_err(AppError::bad_request)?;
    Ok(AppResponse::new(RpcResponse::success(rt), ct))
}
