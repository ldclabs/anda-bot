use anda_core::Principal;
use anda_engine::unix_ms;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Response},
};
use ic_auth_types::ByteArrayB64;
use ic_cose::rand_bytes;
use markdown::{CompileOptions, Options, ParseOptions, to_html, to_html_with_options};
use serde_json::json;
use std::{str::FromStr, sync::LazyLock};

use crate::payload::{Accept, AppError, BearerToken, ContentType, RpcResponse, StringOr};
use crate::space::AppState;
use crate::types::*;

const SKILL_MARKDOWN: &str = include_str!("../SKILL.md");
const WEBSITE_MARKDOWN: &str = include_str!("../WEBSITE.md");
const WEBSITE_CN_MARKDOWN: &str = include_str!("../WEBSITE_cn.md");
const APP_HTML: &str = include_str!("../app.html");
const FAVICON: &[u8] = include_bytes!("../favicon.ico");

pub static WEBSITE: LazyLock<String> =
    LazyLock::new(|| APP_HTML.replace("%sveltekit.body%", &markdown_to_html(WEBSITE_MARKDOWN)));

pub static WEBSITE_CN: LazyLock<String> =
    LazyLock::new(|| APP_HTML.replace("%sveltekit.body%", &markdown_to_html(WEBSITE_CN_MARKDOWN)));

pub async fn favicon() -> Response {
    Response::builder()
        .header("Content-Type", "image/x-icon")
        .body(FAVICON.into())
        .unwrap()
}

pub async fn get_information(State(app): State<AppState>) -> impl IntoResponse {
    let info = json!({
        "name": app.app_name,
        "version": app.app_version,
        "sharding": app.sharding,
         "description": "Hippocampus is a long-term memory system for LLM agents, providing persistent storage and retrieval of knowledge across interactions. It enables agents to remember facts, preferences, relationships, past events, and any other information that can be useful for answering questions and making decisions. Hippocampus organizes memories in a structured way, allowing efficient search and recall based on natural language queries. By using Hippocampus, agents can maintain context and continuity over time, improving their ability to assist users effectively.",
    });

    Json(info)
}

pub async fn get_website(Accept(ct, is_cn): Accept) -> Response {
    match ct {
        ContentType::Markdown(true) => {
            if is_cn {
                ct.response(WEBSITE_CN_MARKDOWN).into_response()
            } else {
                ct.response(WEBSITE_MARKDOWN).into_response()
            }
        }
        _ => {
            if is_cn {
                Html(WEBSITE_CN.replacen("<html lang=\"en\"", "<html lang=\"zh-CN\"", 1))
                    .into_response()
            } else {
                Html(WEBSITE.clone()).into_response()
            }
        }
    }
}

pub async fn get_skill(State(_app): State<AppState>) -> impl IntoResponse {
    ContentType::Markdown(true).response(SKILL_MARKDOWN)
}

/// GET /v1/{space_id}/status
pub async fn get_status(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct, _): Accept,
    BearerToken(token): BearerToken,
) -> Result<impl IntoResponse, AppError> {
    let sid = SpaceId::from_str(&space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let now_ms = unix_ms();
    let t = if token.len() > 60 {
        // 如果 token 存在，永远验证它
        Some(
            app.check_auth(&token, &sid.id, TokenScope::Read, now_ms)
                .map_err(|_| AppError::unauthorized())?,
        )
    } else {
        None
    };

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    if !space.is_public() && t.is_none() {
        // 如果空间不是公开的，且没有验证 CWToken，则验证 SpaceToken
        space
            .verify_space_token(token, TokenScope::Read, now_ms)
            .map_err(|_| AppError::unauthorized())?;
    }

    let rt = space.get_status();
    Ok(ct.response(RpcResponse::success(rt)))
}

/// POST /v1/{space_id}/formation
pub async fn post_formation(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct, _): Accept,
    BearerToken(token): BearerToken,
    body: Bytes,
) -> Result<Response, AppError> {
    let sid = SpaceId::from_str(&space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let input: StringOr<FormationInput> = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let now_ms = unix_ms();
    let t = if token.len() > 60 {
        // 如果 token 存在，永远验证它
        Some(
            app.check_auth(&token, &sid.id, TokenScope::Write, now_ms)
                .map_err(|_| AppError::unauthorized())?,
        )
    } else {
        None
    };

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    if t.is_none() {
        // 如果没有验证 CWToken，则验证 SpaceToken
        space
            .verify_space_token(token, TokenScope::Write, now_ms)
            .map_err(|_| AppError::unauthorized())?;
    }

    // 使用匿名 caller 进行 ingestions 和 queries
    let rt = space
        .ingest(Principal::anonymous(), input)
        .await
        .map_err(AppError::bad_request)?;
    match ct {
        ContentType::Markdown(_) => Ok(ct.response(rt.content).into_response()),
        _ => Ok(ct.response(RpcResponse::success(rt)).into_response()),
    }
}

/// POST /v1/{space_id}/recall
pub async fn post_recall(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct, _): Accept,
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

    let input: StringOr<RecallInput> = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let now_ms = unix_ms();
    let t = if token.len() > 60 {
        // 如果 token 存在，永远验证它
        Some(
            app.check_auth(&token, &sid.id, TokenScope::Read, now_ms)
                .map_err(|_| AppError::unauthorized())?,
        )
    } else {
        None
    };

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    if !space.is_public() && t.is_none() {
        // 如果空间不是公开的，且没有验证 CWToken，则验证 SpaceToken
        space
            .verify_space_token(token, TokenScope::Read, now_ms)
            .map_err(|_| AppError::unauthorized())?;
    }

    // 使用固定的 caller 进行 ingestions 和 queries
    let rt = space
        .query(Principal::anonymous(), input)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(rt)))
}

/// POST /v1/{space_id}/maintenance
pub async fn post_maintenance(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct, _): Accept,
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

    let input: StringOr<MaintenanceInput> = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let now_ms = unix_ms();
    let t = if token.len() > 60 {
        // 如果 token 存在，永远验证它
        Some(
            app.check_auth(&token, &sid.id, TokenScope::Write, now_ms)
                .map_err(|_| AppError::unauthorized())?,
        )
    } else {
        None
    };

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    if t.is_none() {
        // 如果没有验证 CWToken，则验证 SpaceToken
        space
            .verify_space_token(token, TokenScope::Write, now_ms)
            .map_err(|_| AppError::unauthorized())?;
    }

    let rt = space
        .maintenance(Principal::anonymous(), input)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(rt)))
}

/// GET /v1/{space_id}/conversations/{conversation_id}
pub async fn get_conversation(
    State(app): State<AppState>,
    Path((space_id, conversation_id)): Path<(String, String)>,
    Accept(ct, _): Accept,
    BearerToken(token): BearerToken,
) -> Result<impl IntoResponse, AppError> {
    let sid = SpaceId::from_str(&space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }
    let conversation_id: u64 = conversation_id
        .parse()
        .map_err(|_| AppError::bad_request("invalid conversation_id"))?;

    let now_ms = unix_ms();
    let t = if token.len() > 60 {
        // 如果 token 存在，永远验证它
        Some(
            app.check_auth(&token, &sid.id, TokenScope::Read, now_ms)
                .map_err(|_| AppError::unauthorized())?,
        )
    } else {
        None
    };

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    if !space.is_public() && t.is_none() {
        // 如果空间不是公开的，且没有验证 CWToken，则验证 SpaceToken
        space
            .verify_space_token(token, TokenScope::Read, now_ms)
            .map_err(|_| AppError::unauthorized())?;
    }

    let rt = space
        .memory
        .get_conversation(conversation_id)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(rt)))
}

/// GET /v1/{space_id}/conversations
pub async fn list_conversations(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Query(pg): Query<Pagination>,
    Accept(ct, _): Accept,
    BearerToken(token): BearerToken,
) -> Result<impl IntoResponse, AppError> {
    let sid = SpaceId::from_str(&space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let now_ms = unix_ms();
    let t = if token.len() > 60 {
        Some(
            app.check_auth(&token, &sid.id, TokenScope::Read, now_ms)
                .map_err(|_| AppError::unauthorized())?,
        )
    } else {
        None
    };

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    if !space.is_public() && t.is_none() {
        space
            .verify_space_token(token, TokenScope::Read, now_ms)
            .map_err(|_| AppError::unauthorized())?;
    }

    let rt = space
        .memory
        .list_conversations_by_user(&Principal::anonymous(), pg.cursor, pg.limit)
        .await
        .map_err(AppError::bad_request)?;

    Ok(ct.response(RpcResponse {
        result: Some(rt.0),
        error: None,
        next_cursor: rt.1,
    }))
}

/* ===== User management API ===== */

/// GET /v1/{space_id}/management/space_tokens
pub async fn list_space_tokens(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct, _): Accept,
    BearerToken(token): BearerToken,
) -> Result<impl IntoResponse, AppError> {
    let sid = SpaceId::from_str(&space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let now_ms = unix_ms();
    let _ = app
        .check_auth(&token, &sid.id, TokenScope::Read, now_ms)
        .map_err(|_| AppError::unauthorized())?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    let rt = space.list_space_tokens().map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(rt)))
}

/// POST /v1/{space_id}/management/add_space_token
pub async fn add_space_token(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct, _): Accept,
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

    let now_ms = unix_ms();
    let _ = app
        .check_auth(&token, &sid.id, TokenScope::Write, now_ms)
        .map_err(|_| AppError::unauthorized())?;

    let input: AddSpaceTokenInput = ct
        .parse_body(&body)
        .map_err(AppError::bad_request)?
        .value()
        .map_err(|_| AppError::bad_request("invalid input"))?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    let data: [u8; 20] = rand_bytes();
    let token = format!("ST{}", ByteArrayB64(data));
    space
        .add_space_token(token.clone(), input.scope, now_ms)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(token)))
}

/// POST /v1/{space_id}/management/revoke_space_token
pub async fn revoke_space_token(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct, _): Accept,
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

    let now_ms = unix_ms();
    let _ = app
        .check_auth(&token, &sid.id, TokenScope::Write, now_ms)
        .map_err(|_| AppError::unauthorized())?;

    let input: RevokeSpaceTokenInput = ct
        .parse_body(&body)
        .map_err(AppError::bad_request)?
        .value()
        .map_err(|_| AppError::bad_request("invalid input"))?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    let rt = space
        .revoke_space_token(&input.token)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(rt)))
}

/// POST /v1/{space_id}/management/set_public
pub async fn set_public(
    State(app): State<AppState>,
    Path(space_id): Path<String>,
    Accept(ct, _): Accept,
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

    let now_ms = unix_ms();
    let _ = app
        .check_auth(&token, &sid.id, TokenScope::Write, now_ms)
        .map_err(|_| AppError::unauthorized())?;

    let input: SetSpacePublicInput = ct
        .parse_body(&body)
        .map_err(AppError::bad_request)?
        .value()
        .map_err(|_| AppError::bad_request("invalid input"))?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    space
        .set_public(input.public, now_ms)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(true)))
}

/* ===== Admin API ===== */

/// POST /admin/create_space
pub async fn create_space(
    State(app): State<AppState>,
    Accept(ct, _): Accept,
    BearerToken(token): BearerToken,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let now_ms = unix_ms();
    let token = app
        .check_admin(&token, "*", TokenScope::Write, now_ms)
        .map_err(|_| AppError::unauthorized())?;

    let input: CreateOrUpdateSpaceInput = ct
        .parse_body(&body)
        .map_err(AppError::bad_request)?
        .value()
        .map_err(|_| AppError::bad_request("invalid input"))?;

    let sid = SpaceId::from_str(&input.space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let rt = app
        .admin_create_space(token.user, input.user, sid.id, input.tier, now_ms)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(rt)))
}

/// POST /admin/update_space_tier
pub async fn update_space_tier(
    State(app): State<AppState>,
    Accept(ct, _): Accept,
    BearerToken(token): BearerToken,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let now_ms = unix_ms();
    let _ = app
        .check_admin(&token, "*", TokenScope::Write, now_ms)
        .map_err(|_| AppError::unauthorized())?;

    let input: CreateOrUpdateSpaceInput = ct
        .parse_body(&body)
        .map_err(AppError::bad_request)?
        .value()
        .map_err(|_| AppError::bad_request("invalid input"))?;

    let sid = SpaceId::from_str(&input.space_id).map_err(AppError::bad_request)?;
    if sid.sharding != app.sharding {
        return Err(AppError::bad_request(format!(
            "space_id sharding {} does not match server sharding {}",
            sid.sharding, app.sharding
        )));
    }

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;

    let rt = space
        .admin_update_tier(input.tier, now_ms)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(rt)))
}

fn markdown_to_html(md: &str) -> String {
    to_html_with_options(
        md,
        &Options {
            parse: ParseOptions::gfm(),
            compile: CompileOptions {
                allow_any_img_src: true,
                allow_dangerous_html: true,
                allow_dangerous_protocol: true,
                gfm_tagfilter: false,
                ..CompileOptions::gfm()
            },
        },
    )
    .unwrap_or_else(|_| to_html(md))
}
