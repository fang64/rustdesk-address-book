use axum::{
    extract::{Path, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::error::ApiError;
use crate::models::tag::*;
use crate::routes::peers::resolve_ab_guid;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/ab/tags/{guid}", get(get_tags).post(post_tags))
        .route("/api/ab/tag/add/{guid}", post(add_tag))
        .route("/api/ab/tag/rename/{guid}", put(rename_tag))
        .route("/api/ab/tag/update/{guid}", put(update_tag_color))
        .route("/api/ab/tag/{guid}", delete(delete_tags))
}

async fn get_tags(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(guid): Path<String>,
) -> Result<Json<TagsResponse>, ApiError> {
    let guid = resolve_ab_guid(&state.db, claims.user_id, &guid).await?;

    let tags = sqlx::query_as::<_, Tag>(
        "SELECT * FROM tags WHERE ab_guid = ? ORDER BY name",
    )
    .bind(&guid)
    .fetch_all(&state.db)
    .await?;

    let total = tags.len() as i64;
    let data: Vec<TagPayload> = tags
        .into_iter()
        .map(|t| TagPayload {
            name: t.name,
            color: t.color,
        })
        .collect();

    Ok(Json(TagsResponse { data, total }))
}

// RustDesk 1.5.0 expects an array from POST, while the web app uses the
// paginated GET response.
async fn post_tags(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(guid): Path<String>,
) -> Result<Json<Vec<TagPayload>>, ApiError> {
    let guid = resolve_ab_guid(&state.db, claims.user_id, &guid).await?;
    let tags = sqlx::query_as::<_, Tag>(
        "SELECT * FROM tags WHERE ab_guid = ? ORDER BY name",
    )
    .bind(&guid)
    .fetch_all(&state.db)
    .await?;
    let data = tags.into_iter().map(|t| TagPayload {
        name: t.name,
        color: t.color,
    }).collect();
    Ok(Json(data))
}

async fn add_tag(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(guid): Path<String>,
    Json(req): Json<AddTagRequest>,
) -> Result<Json<Value>, ApiError> {
    let guid = resolve_ab_guid(&state.db, claims.user_id, &guid).await?;

    sqlx::query("INSERT OR IGNORE INTO tags (ab_guid, name, color) VALUES (?, ?, ?)")
        .bind(&guid)
        .bind(&req.name)
        .bind(req.color)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({})))
}

async fn rename_tag(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(guid): Path<String>,
    Json(req): Json<RenameTagRequest>,
) -> Result<Json<Value>, ApiError> {
    let guid = resolve_ab_guid(&state.db, claims.user_id, &guid).await?;

    sqlx::query("UPDATE tags SET name = ? WHERE ab_guid = ? AND name = ?")
        .bind(&req.new)
        .bind(&guid)
        .bind(&req.old)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({})))
}

async fn update_tag_color(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(guid): Path<String>,
    Json(req): Json<UpdateTagColorRequest>,
) -> Result<Json<Value>, ApiError> {
    let guid = resolve_ab_guid(&state.db, claims.user_id, &guid).await?;

    sqlx::query("UPDATE tags SET color = ? WHERE ab_guid = ? AND name = ?")
        .bind(req.color)
        .bind(&guid)
        .bind(&req.name)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({})))
}

async fn delete_tags(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(guid): Path<String>,
    Json(req): Json<DeleteTagRequest>,
) -> Result<Json<Value>, ApiError> {
    let guid = resolve_ab_guid(&state.db, claims.user_id, &guid).await?;

    let mut names = req.names;
    if let Some(name) = req.name {
        names.push(name);
    }

    for name in &names {
        // Remove peer_tags associations first
        sqlx::query(
            "DELETE FROM peer_tags WHERE tag_id IN (SELECT id FROM tags WHERE ab_guid = ? AND name = ?)",
        )
        .bind(&guid)
        .bind(name)
        .execute(&state.db)
        .await?;

        sqlx::query("DELETE FROM tags WHERE ab_guid = ? AND name = ?")
            .bind(&guid)
            .bind(name)
            .execute(&state.db)
            .await?;
    }

    Ok(Json(json!({})))
}
