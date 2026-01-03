use crate::db::{self, DbPool};
use crate::error::AppError;
use crate::models::Summary;
use axum::{extract::State, Json};

pub async fn get_summary(State(pool): State<DbPool>) -> Result<Json<Summary>, AppError> {
    let summary = db::get_summary(&pool)?;
    Ok(Json(summary))
}
