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
#[allow(dead_code)]
pub struct OAuthCallbackQuery {
    pub code: String,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InitiateOAuthRequest {
    pub client_id: String,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize)]
pub struct OAuthUrlResponse {
    pub auth_url: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct CompleteOAuthRequest {
    pub code: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
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

/// Initiate OAuth flow - returns authorization URL for user to visit
pub async fn initiate_oauth(
    Json(req): Json<InitiateOAuthRequest>,
) -> Result<Json<OAuthUrlResponse>, AppError> {
    // Generate a random state for CSRF protection
    let state = format!("{}", chrono::Utc::now().timestamp());

    let auth_url = GmailScanner::get_auth_url(&req.client_id, &req.redirect_uri, &state);

    Ok(Json(OAuthUrlResponse { auth_url, state }))
}

/// Complete OAuth flow - exchange code for token and save account
pub async fn complete_oauth(
    State(pool): State<DbPool>,
    Json(req): Json<CompleteOAuthRequest>,
) -> Result<Json<GmailConnectionResponse>, AppError> {
    // Check if account already exists
    if let Some(_existing) = db::get_gmail_account(&pool, &req.email)? {
        return Err(AppError::InvalidInput(format!(
            "Gmail account {} is already connected",
            req.email
        )));
    }

    // Exchange authorization code for tokens
    let token_response = GmailScanner::exchange_code(
        &req.client_id,
        &req.client_secret,
        &req.code,
        &req.redirect_uri,
    )
    .await?;

    // Calculate token expiry timestamp
    let expires_at = chrono::Utc::now().timestamp() + token_response.expires_in;

    // Create Gmail account record
    let gmail_account = GmailAccount {
        id: None,
        email: req.email.clone(),
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token.unwrap_or_default(),
        token_expiry: expires_at,
        created_at: None,
        last_scan_at: None,
    };

    // Save to database
    let account_id = db::save_gmail_account(&pool, &gmail_account)?;

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

    // Check if token is expired and needs refresh
    let now = chrono::Utc::now().timestamp();
    let access_token = if now >= account.token_expiry {
        // Token expired, need to refresh
        // Note: In production, you'd need to store client_id and client_secret
        // For now, return error asking user to reconnect
        return Err(AppError::Internal(
            "Access token expired. Please reconnect your Gmail account.".to_string(),
        ));
    } else {
        account.access_token.clone()
    };

    // Create scanner
    let scanner = GmailScanner::new(access_token, account.email.clone())?;

    // Get scan configuration
    let config = req.config.unwrap_or_default();

    // Perform scan
    let (mut result, discoveries) = scanner.scan(&config).await?;

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

    Ok(Json(json!({
        "success": true,
        "message": "Gmail account disconnected"
    })))
}
