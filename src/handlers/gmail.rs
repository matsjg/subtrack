use crate::db::{self, DbPool};
use crate::error::AppError;
use crate::gmail::GmailScanner;
use crate::models::{
    AcceptDiscoveryRequest, DiscoveredSubscription, DiscoveryStatus, GmailAccount, ScanResult,
    ScannerConfig,
};
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct ConnectGmailRequest {
    pub client_id: String,
    pub client_secret: String,
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct GmailConnectionResponse {
    pub success: bool,
    pub message: String,
    pub account_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    pub config: Option<ScannerConfig>,
}

/// Connect a Gmail account via OAuth
pub async fn connect_gmail(
    State(pool): State<DbPool>,
    Json(req): Json<ConnectGmailRequest>,
) -> Result<Json<GmailConnectionResponse>, AppError> {
    // Check if account already exists
    if let Some(_existing) = db::get_gmail_account(&pool, &req.email)? {
        return Err(AppError::InvalidInput(format!(
            "Gmail account {} is already connected",
            req.email
        )));
    }

    // Initialize Gmail scanner and authenticate
    let (_scanner, mut gmail_account) =
        GmailScanner::new(&req.client_id, &req.client_secret, &req.email)
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to authenticate with Gmail: {}", e))
            })?;

    // Save account to database
    let account_id = db::save_gmail_account(&pool, &gmail_account)?;
    gmail_account.id = Some(account_id);

    tracing::info!("Successfully connected Gmail account: {}", req.email);

    Ok(Json(GmailConnectionResponse {
        success: true,
        message: format!("Successfully connected Gmail account {}", req.email),
        account_id: Some(account_id),
    }))
}

/// List connected Gmail accounts
pub async fn list_gmail_accounts(
    State(pool): State<DbPool>,
) -> Result<Json<Vec<GmailAccount>>, AppError> {
    let accounts = db::list_gmail_accounts(&pool)?;
    // Don't expose tokens in API response
    let sanitized: Vec<GmailAccount> = accounts
        .into_iter()
        .map(|mut acc| {
            acc.access_token = "***".to_string();
            acc.refresh_token = "***".to_string();
            acc
        })
        .collect();
    Ok(Json(sanitized))
}

/// Scan Gmail inbox for subscriptions
pub async fn scan_gmail(
    State(pool): State<DbPool>,
    Path(account_id): Path<i64>,
    Json(req): Json<ScanRequest>,
) -> Result<Json<ScanResult>, AppError> {
    // Get Gmail account from database
    let accounts = db::list_gmail_accounts(&pool)?;
    let account = accounts
        .into_iter()
        .find(|a| a.id == Some(account_id))
        .ok_or_else(|| AppError::NotFound(format!("Gmail account {} not found", account_id)))?;

    // Note: In a real implementation, we would need to properly reconstruct the scanner
    // with saved OAuth tokens. For this MVP, we'll return a placeholder error message.

    return Err(AppError::Internal(
        "Gmail scanning requires proper OAuth token management. \
         Please refer to the documentation for setting up Google Cloud credentials."
            .to_string(),
    ));

    // This is the intended implementation (commented out for now):
    /*
    let scanner = GmailScanner::from_account(&account).await?;
    let config = req.config.unwrap_or_default();

    // Perform scan
    let mut result = scanner.scan(&config).await?;

    // Save discovered subscriptions to database
    let mut new_count = 0;
    let mut updated_count = 0;

    for mut discovery in discoveries {
        discovery.gmail_account_id = account_id;

        // Check if this discovery already exists
        let existing = db::list_discovered_subscriptions(&pool, account_id, None)?
            .into_iter()
            .find(|d| d.sender_email == discovery.sender_email);

        if existing.is_some() {
            updated_count += 1;
        } else {
            new_count += 1;
        }

        db::upsert_discovered_subscription(&pool, &discovery)?;
    }

    // Update scan time
    db::update_gmail_scan_time(&pool, account_id)?;

    result.new_discoveries = new_count;
    result.updated_discoveries = updated_count;

    Ok(Json(result))
    */
}

/// List discovered subscriptions
pub async fn list_discoveries(
    State(pool): State<DbPool>,
    Path(account_id): Path<i64>,
) -> Result<Json<Vec<DiscoveredSubscription>>, AppError> {
    // Only show pending discoveries by default
    let discoveries =
        db::list_discovered_subscriptions(&pool, account_id, Some(DiscoveryStatus::Pending))?;
    Ok(Json(discoveries))
}

/// Accept a discovered subscription and create it
pub async fn accept_discovery(
    State(pool): State<DbPool>,
    Path(discovery_id): Path<i64>,
    Json(req): Json<AcceptDiscoveryRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Validate the request
    if req.name.trim().is_empty() {
        return Err(AppError::InvalidInput("Name cannot be empty".to_string()));
    }

    if req.cost < 0.0 {
        return Err(AppError::InvalidInput(
            "Cost cannot be negative".to_string(),
        ));
    }

    // Validate date format
    chrono::NaiveDate::parse_from_str(&req.renewal_date, "%Y-%m-%d").map_err(|_| {
        AppError::InvalidInput("Invalid date format. Use YYYY-MM-DD".to_string())
    })?;

    // Create subscription
    let subscription_id = db::create_subscription(
        &pool,
        &req.name,
        req.cost,
        &req.billing_cycle,
        &req.renewal_date,
        req.category.as_deref(),
        req.notes.as_deref(),
        req.reminder_days.unwrap_or(7),
    )?;

    // Update discovery status
    db::update_discovery_status(
        &pool,
        discovery_id,
        DiscoveryStatus::Accepted,
        Some(subscription_id),
    )?;

    Ok(Json(json!({
        "success": true,
        "message": "Subscription created from discovery",
        "subscription_id": subscription_id
    })))
}

/// Ignore a discovered subscription
pub async fn ignore_discovery(
    State(pool): State<DbPool>,
    Path(discovery_id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let updated = db::update_discovery_status(&pool, discovery_id, DiscoveryStatus::Ignored, None)?;

    if !updated {
        return Err(AppError::NotFound(format!(
            "Discovery {} not found",
            discovery_id
        )));
    }

    Ok(Json(json!({
        "success": true,
        "message": "Discovery ignored"
    })))
}

/// Delete a discovered subscription
pub async fn delete_discovery(
    State(pool): State<DbPool>,
    Path(discovery_id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = db::delete_discovered_subscription(&pool, discovery_id)?;

    if !deleted {
        return Err(AppError::NotFound(format!(
            "Discovery {} not found",
            discovery_id
        )));
    }

    Ok(Json(json!({
        "success": true,
        "message": "Discovery deleted"
    })))
}

/// Disconnect Gmail account
pub async fn disconnect_gmail(
    State(pool): State<DbPool>,
    Path(account_id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = db::delete_gmail_account(&pool, account_id)?;

    if !deleted {
        return Err(AppError::NotFound(format!(
            "Gmail account {} not found",
            account_id
        )));
    }

    // Clean up token cache file if it exists
    let _ = std::fs::remove_file("token_cache.json");

    Ok(Json(json!({
        "success": true,
        "message": "Gmail account disconnected"
    })))
}
