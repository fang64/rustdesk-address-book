use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::error::ApiError;
use crate::models::group::*;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/groups", get(list_groups).post(create_group))
        .route("/api/groups/{id}", put(update_group).delete(delete_group))
        .route("/api/device-group/accessible", get(empty_client_device_groups))
}

// Community hbbs does not have device groups. Return the paginated JSON shape
// expected by RustDesk instead of falling through to the web app's HTML page.
async fn empty_client_device_groups(AuthUser(_claims): AuthUser) -> Json<Value> {
    Json(json!({ "data": [], "total": 0 }))
}

fn require_admin(claims: &crate::auth::jwt::Claims) -> Result<(), ApiError> {
    if !claims.is_admin {
        return Err(ApiError::Forbidden("Admin access required".to_string()));
    }
    Ok(())
}

async fn list_groups(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Value>, ApiError> {
    require_admin(&claims)?;

    let groups = sqlx::query_as::<_, Group>("SELECT * FROM groups ORDER BY id")
        .fetch_all(&state.db)
        .await?;

    let items: Vec<GroupListItem> = groups
        .into_iter()
        .map(|g| GroupListItem {
            id: g.id,
            name: g.name,
            note: g.note,
            created_at: g.created_at,
        })
        .collect();

    let total = items.len();
    Ok(Json(json!({ "data": items, "total": total })))
}

async fn create_group(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&claims)?;

    sqlx::query("INSERT INTO groups (name, note) VALUES (?, ?)")
        .bind(&req.name)
        .bind(&req.note)
        .execute(&state.db)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.message().contains("UNIQUE") => {
                ApiError::Conflict(format!("Group '{}' already exists", req.name))
            }
            _ => ApiError::Internal(e.to_string()),
        })?;

    Ok(Json(json!({})))
}

async fn update_group(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&claims)?;

    if let Some(name) = req.get("name").and_then(|v| v.as_str()) {
        sqlx::query("UPDATE groups SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    if let Some(note) = req.get("note").and_then(|v| v.as_str()) {
        sqlx::query("UPDATE groups SET note = ? WHERE id = ?")
            .bind(note)
            .bind(id)
            .execute(&state.db)
            .await?;
    }

    Ok(Json(json!({})))
}

async fn delete_group(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&claims)?;

    sqlx::query("DELETE FROM groups WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({})))
}
