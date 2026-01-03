use crate::db::{self, DbPool};
use crate::error::AppError;
use crate::models::{CreateSubscriptionRequest, Subscription, SubscriptionResponse, UpdateSubscriptionRequest};
use axum::{
    extract::{Path, State},
    Json,
};

pub async fn list_subscriptions(
    State(pool): State<DbPool>,
) -> Result<Json<Vec<Subscription>>, AppError> {
    let subscriptions = db::list_subscriptions(&pool)?;
    Ok(Json(subscriptions))
}

pub async fn get_subscription(
    State(pool): State<DbPool>,
    Path(id): Path<i64>,
) -> Result<Json<Subscription>, AppError> {
    let subscription = db::get_subscription(&pool, id)?
        .ok_or_else(|| AppError::NotFound(format!("Subscription {} not found", id)))?;
    Ok(Json(subscription))
}

pub async fn create_subscription(
    State(pool): State<DbPool>,
    Json(req): Json<CreateSubscriptionRequest>,
) -> Result<Json<SubscriptionResponse>, AppError> {
    // Validate the request
    if req.name.trim().is_empty() {
        return Err(AppError::InvalidInput("Name cannot be empty".to_string()));
    }

    if req.cost < 0.0 {
        return Err(AppError::InvalidInput("Cost cannot be negative".to_string()));
    }

    // Validate date format
    chrono::NaiveDate::parse_from_str(&req.renewal_date, "%Y-%m-%d")
        .map_err(|_| AppError::InvalidInput("Invalid date format. Use YYYY-MM-DD".to_string()))?;

    let id = db::create_subscription(
        &pool,
        &req.name,
        req.cost,
        &req.billing_cycle,
        &req.renewal_date,
        req.category.as_deref(),
        req.notes.as_deref(),
        req.reminder_days.unwrap_or(7),
    )?;

    Ok(Json(SubscriptionResponse {
        id,
        message: Some("Subscription created".to_string()),
    }))
}

pub async fn update_subscription(
    State(pool): State<DbPool>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateSubscriptionRequest>,
) -> Result<Json<SubscriptionResponse>, AppError> {
    // Validate inputs
    if let Some(ref name) = req.name {
        if name.trim().is_empty() {
            return Err(AppError::InvalidInput("Name cannot be empty".to_string()));
        }
    }

    if let Some(cost) = req.cost {
        if cost < 0.0 {
            return Err(AppError::InvalidInput("Cost cannot be negative".to_string()));
        }
    }

    if let Some(ref date) = req.renewal_date {
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| AppError::InvalidInput("Invalid date format. Use YYYY-MM-DD".to_string()))?;
    }

    let updated = db::update_subscription(
        &pool,
        id,
        req.name.as_deref(),
        req.cost,
        req.billing_cycle.as_ref(),
        req.renewal_date.as_deref(),
        req.category.as_ref().map(|s| Some(s.as_str())),
        req.notes.as_ref().map(|s| Some(s.as_str())),
        req.reminder_days,
        req.status.as_ref(),
    )?;

    if !updated {
        return Err(AppError::NotFound(format!("Subscription {} not found", id)));
    }

    Ok(Json(SubscriptionResponse {
        id,
        message: Some("Subscription updated".to_string()),
    }))
}

pub async fn delete_subscription(
    State(pool): State<DbPool>,
    Path(id): Path<i64>,
) -> Result<Json<SubscriptionResponse>, AppError> {
    let deleted = db::delete_subscription(&pool, id)?;

    if !deleted {
        return Err(AppError::NotFound(format!("Subscription {} not found", id)));
    }

    Ok(Json(SubscriptionResponse {
        id,
        message: Some("Subscription deleted".to_string()),
    }))
}

pub async fn cancel_subscription(
    State(pool): State<DbPool>,
    Path(id): Path<i64>,
) -> Result<Json<SubscriptionResponse>, AppError> {
    let updated = db::cancel_subscription(&pool, id)?;

    if !updated {
        return Err(AppError::NotFound(format!("Subscription {} not found", id)));
    }

    Ok(Json(SubscriptionResponse {
        id,
        message: Some("Subscription canceled".to_string()),
    }))
}

pub async fn reactivate_subscription(
    State(pool): State<DbPool>,
    Path(id): Path<i64>,
) -> Result<Json<SubscriptionResponse>, AppError> {
    let updated = db::reactivate_subscription(&pool, id)?;

    if !updated {
        return Err(AppError::NotFound(format!("Subscription {} not found", id)));
    }

    Ok(Json(SubscriptionResponse {
        id,
        message: Some("Subscription reactivated".to_string()),
    }))
}
