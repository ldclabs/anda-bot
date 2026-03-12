use anda_core::Principal;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
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

pub static WEBSITE: LazyLock<String> = LazyLock::new(|| {
    let body = to_html_with_options(
        WEBSITE_MARKDOWN,
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
    .unwrap_or_else(|_| to_html(WEBSITE_MARKDOWN));
    APP_HTML.replace("%sveltekit.body%", &body)
});

pub static WEBSITE_CN: LazyLock<String> = LazyLock::new(|| {
    let body = to_html_with_options(
        WEBSITE_CN_MARKDOWN,
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
    .unwrap_or_else(|_| to_html(WEBSITE_CN_MARKDOWN));
    APP_HTML.replace("%sveltekit.body%", &body)
});

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

/// POST /admin/create_space
pub async fn create_space(
    State(app): State<AppState>,
    Accept(ct, _): Accept,
    BearerToken(token): BearerToken,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let token = app
        .check_admin(&token, "*", "write")
        .map_err(|_| AppError::unauthorized())?;

    let input: StringOr<CreateSpaceInput> = ct.parse_body(&body).map_err(AppError::bad_request)?;
    let input = input
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
        .create_space(token.user, input.user, sid.id)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(rt)))
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

    app.check_auth(&token, &sid.id, "read")
        .map_err(|_| AppError::unauthorized())?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;
    let rt = space.get_status().await;
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

    let _ = app
        .check_auth(&token, &sid.id, "write")
        .map_err(|_| AppError::unauthorized())?;
    let input: StringOr<FormationInput> = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;
    // 使用固定的 caller 进行 ingestions 和 queries
    let rt = space
        .ingest(Principal::management_canister(), input)
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

    let _ = app
        .check_auth(&token, &sid.id, "read")
        .map_err(|_| AppError::unauthorized())?;
    let input: StringOr<RecallInput> = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;
    // 使用固定的 caller 进行 ingestions 和 queries
    let rt = space
        .query(Principal::management_canister(), input)
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

    let _ = app
        .check_auth(&token, &sid.id, "write")
        .map_err(|_| AppError::unauthorized())?;
    let input: StringOr<MaintenanceInput> = ct.parse_body(&body).map_err(AppError::bad_request)?;

    let space = app
        .load_space(&sid.id)
        .await
        .map_err(AppError::bad_request)?;
    let rt = space
        .maintenance(Principal::management_canister(), input)
        .await
        .map_err(AppError::bad_request)?;
    Ok(ct.response(RpcResponse::success(rt)))
}
