use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::error::ApiError;
use crate::models::peer::*;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/ab/peers", get(get_peers).post(get_peers))
        .route("/api/ab/peer/add/{guid}", post(add_peer))
        .route("/api/ab/peer/update/{guid}", put(update_peer))
        .route("/api/ab/peer/{guid}", delete(delete_peers))
        .route("/api/peers", get(empty_client_group_peers))
}

// Community hbbs does not expose the Pro group peer directory.
async fn empty_client_group_peers(AuthUser(_claims): AuthUser) -> Json<Value> {
    Json(json!({ "data": [], "total": 0 }))
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct PeersQuery {
    #[serde(default)]
    pub current: i64,
    #[serde(default = "default_page_size")]
    pub pageSize: i64,
    #[serde(default)]
    pub ab: String,
}

fn default_page_size() -> i64 {
    100
}

/// Verify the user has access to the given address book guid.
/// Returns the guid of the personal AB if `ab` is empty.
pub async fn resolve_ab_guid(
    db: &sqlx::SqlitePool,
    user_id: i64,
    ab_guid: &str,
) -> Result<String, ApiError> {
    if ab_guid.is_empty() {
        // Use personal AB
        let guid: Option<String> = sqlx::query_scalar(
            "SELECT guid FROM address_books WHERE owner_id = ? AND is_personal = TRUE",
        )
        .bind(user_id)
        .fetch_optional(db)
        .await?;
        return guid.ok_or_else(|| ApiError::NotFound("No personal address book found".to_string()));
    }

    // Check ownership or share access
    let has_access: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM address_books ab
         WHERE ab.guid = ? AND (
             ab.owner_id = ?
             OR EXISTS (SELECT 1 FROM ab_shares s WHERE s.ab_guid = ab.guid AND (s.user_id = ? OR s.group_id IN (SELECT group_id FROM user_groups WHERE user_id = ?)))
         )",
    )
    .bind(ab_guid)
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .fetch_one(db)
    .await?;

    if !has_access {
        return Err(ApiError::Forbidden("Access denied to this address book".to_string()));
    }

    Ok(ab_guid.to_string())
}

async fn get_peers(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(query): Query<PeersQuery>,
) -> Result<Json<PeersResponse>, ApiError> {
    let guid = resolve_ab_guid(&state.db, claims.user_id, &query.ab).await?;

    let offset = if query.current > 0 {
        (query.current - 1) * query.pageSize
    } else {
        0
    };

    let peers = sqlx::query_as::<_, Peer>(
        "SELECT * FROM peers WHERE ab_guid = ? ORDER BY rustdesk_id LIMIT ? OFFSET ?",
    )
    .bind(&guid)
    .bind(query.pageSize)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM peers WHERE ab_guid = ?")
            .bind(&guid)
            .fetch_one(&state.db)
            .await?;

    let mut payloads = Vec::new();
    for peer in &peers {
        let tags: Vec<String> = sqlx::query_scalar(
            "SELECT t.name FROM tags t JOIN peer_tags pt ON t.id = pt.tag_id WHERE pt.peer_id = ?",
        )
        .bind(peer.id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        payloads.push(PeerPayload {
            id: peer.rustdesk_id.clone(),
            hash: peer.hash.clone(),
            username: peer.username.clone(),
            hostname: peer.hostname.clone(),
            platform: peer.platform.clone(),
            alias: peer.alias.clone(),
            tags,
            note: peer.note.clone(),
        });
    }

    Ok(Json(PeersResponse {
        data: payloads,
        total,
    }))
}

async fn add_peer(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(guid): Path<String>,
    Json(req): Json<AddPeerRequest>,
) -> Result<Json<Value>, ApiError> {
    let guid = resolve_ab_guid(&state.db, claims.user_id, &guid).await?;

    sqlx::query(
        "INSERT INTO peers (ab_guid, rustdesk_id, hash, username, hostname, platform, alias, note)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(ab_guid, rustdesk_id) DO UPDATE SET
             hash = excluded.hash,
             username = excluded.username,
             hostname = excluded.hostname,
             platform = excluded.platform,
             alias = excluded.alias,
             note = excluded.note,
             updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&guid)
    .bind(&req.id)
    .bind(&req.hash)
    .bind(&req.username)
    .bind(&req.hostname)
    .bind(&req.platform)
    .bind(&req.alias)
    .bind(&req.note)
    .execute(&state.db)
    .await?;

    // Update tags if provided
    if !req.tags.is_empty() {
        // Get the actual peer id (might differ if it was an update)
        let actual_peer_id: i64 = sqlx::query_scalar(
            "SELECT id FROM peers WHERE ab_guid = ? AND rustdesk_id = ?",
        )
        .bind(&guid)
        .bind(&req.id)
        .fetch_one(&state.db)
        .await?;

        sqlx::query("DELETE FROM peer_tags WHERE peer_id = ?")
            .bind(actual_peer_id)
            .execute(&state.db)
            .await?;

        for tag_name in &req.tags {
            let tag_id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM tags WHERE ab_guid = ? AND name = ?",
            )
            .bind(&guid)
            .bind(tag_name)
            .fetch_optional(&state.db)
            .await?;

            if let Some(tag_id) = tag_id {
                sqlx::query("INSERT OR IGNORE INTO peer_tags (peer_id, tag_id) VALUES (?, ?)")
                    .bind(actual_peer_id)
                    .bind(tag_id)
                    .execute(&state.db)
                    .await?;
            }
        }
    }

    Ok(Json(json!({})))
}

async fn update_peer(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(guid): Path<String>,
    Json(_req): Json<UpdatePeerRequest>,
) -> Result<Json<Value>, ApiError> {
    let _guid = resolve_ab_guid(&state.db, claims.user_id, &guid).await?;

    // The client typically uses add_peer with upsert semantics for updates.
    // This endpoint exists for compatibility.

    Ok(Json(json!({})))
}

async fn delete_peers(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(guid): Path<String>,
    Json(req): Json<DeletePeersRequest>,
) -> Result<Json<Value>, ApiError> {
    let guid = resolve_ab_guid(&state.db, claims.user_id, &guid).await?;

    let mut ids_to_delete = req.ids;
    if let Some(id) = req.id {
        ids_to_delete.push(id);
    }

    for rustdesk_id in &ids_to_delete {
        // Delete peer_tags first
        sqlx::query(
            "DELETE FROM peer_tags WHERE peer_id IN (SELECT id FROM peers WHERE ab_guid = ? AND rustdesk_id = ?)",
        )
        .bind(&guid)
        .bind(rustdesk_id)
        .execute(&state.db)
        .await?;

        sqlx::query("DELETE FROM peers WHERE ab_guid = ? AND rustdesk_id = ?")
            .bind(&guid)
            .bind(rustdesk_id)
            .execute(&state.db)
            .await?;
    }

    Ok(Json(json!({})))
}
