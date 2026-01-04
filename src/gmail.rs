use crate::error::AppError;
use crate::models::{DiscoveredSubscription, DiscoveryStatus, ScanResult, ScannerConfig};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1";
const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const OAUTH_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    pub refresh_token: Option<String>,
    pub token_type: String,
}

#[derive(Debug, Deserialize)]
struct GmailListResponse {
    messages: Option<Vec<MessageRef>>,
    #[allow(dead_code)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageRef {
    id: String,
    #[allow(dead_code)]
    thread_id: String,
}

#[derive(Debug, Deserialize)]
struct GmailMessage {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    thread_id: String,
    payload: Option<MessagePart>,
    #[serde(rename = "internalDate")]
    internal_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessagePart {
    headers: Option<Vec<MessageHeader>>,
}

#[derive(Debug, Deserialize)]
struct MessageHeader {
    name: String,
    value: String,
}

pub struct GmailScanner {
    client: Client,
    access_token: String,
    #[allow(dead_code)]
    account_email: String,
}

impl GmailScanner {
    /// Create a new Gmail scanner with an access token
    pub fn new(access_token: String, account_email: String) -> Result<Self, AppError> {
        let client = Client::builder()
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            access_token,
            account_email,
        })
    }

    /// Generate OAuth authorization URL for user to visit
    pub fn get_auth_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&state={}&prompt=consent",
            OAUTH_AUTH_URL,
            urlencoding::encode(client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode("https://www.googleapis.com/auth/gmail.readonly"),
            urlencoding::encode(state)
        )
    }

    /// Exchange authorization code for access token
    pub async fn exchange_code(
        client_id: &str,
        client_secret: &str,
        code: &str,
        redirect_uri: &str,
    ) -> Result<OAuthTokenResponse, AppError> {
        let client = Client::new();
        let params = [
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ];

        let response = client
            .post(OAUTH_TOKEN_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("OAuth token exchange failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "OAuth token exchange failed: {}",
                error_text
            )));
        }

        let token_response: OAuthTokenResponse = response
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse token response: {}", e)))?;

        Ok(token_response)
    }

    /// Refresh an access token using refresh token
    #[allow(dead_code)]
    pub async fn refresh_token(
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<OAuthTokenResponse, AppError> {
        let client = Client::new();
        let params = [
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "refresh_token"),
        ];

        let response = client
            .post(OAUTH_TOKEN_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Token refresh failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Token refresh failed: {}",
                error_text
            )));
        }

        let token_response: OAuthTokenResponse = response
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse token response: {}", e)))?;

        Ok(token_response)
    }

    /// Scan Gmail inbox for subscription-related emails
    pub async fn scan(&self, config: &ScannerConfig) -> Result<(ScanResult, Vec<DiscoveredSubscription>), AppError> {
        let start_time = Instant::now();
        let mut all_discoveries = Vec::new();

        // Default search queries for subscription detection
        let default_queries = vec![
            "unsubscribe",
            "\"manage subscription\"",
            "\"cancel subscription\"",
            "\"your receipt\"",
            "\"payment received\"",
            "renewal",
            "from:noreply OR from:no-reply",
        ];

        let queries: Vec<&str> = default_queries
            .iter()
            .map(|s| *s)
            .chain(config.custom_queries.iter().map(|s| s.as_str()))
            .collect();

        let mut total_emails = 0;

        for query in queries {
            match self
                .fetch_messages(query, config.max_results_per_query)
                .await
            {
                Ok(messages) => {
                    total_emails += messages.len() as u32;
                    all_discoveries.extend(messages);
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch messages for query '{}': {}", query, e);
                }
            }
        }

        // Parse and deduplicate
        let discoveries = self.parse_and_deduplicate(all_discoveries, config);

        let scan_duration = start_time.elapsed();

        let result = ScanResult {
            total_emails_scanned: total_emails,
            new_discoveries: discoveries.len() as u32,
            updated_discoveries: 0, // Will be set by database layer
            scan_duration_ms: scan_duration.as_millis() as u64,
        };

        Ok((result, discoveries))
    }

    /// Fetch messages matching a query
    async fn fetch_messages(
        &self,
        query: &str,
        max_results: u32,
    ) -> Result<Vec<DiscoveredSubscription>, AppError> {
        let url = format!("{}/users/me/messages", GMAIL_API_BASE);

        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[("q", query), ("maxResults", &max_results.to_string())])
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Gmail API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Gmail API error {}: {}",
                status, error_text
            )));
        }

        let list_response: GmailListResponse = response
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse Gmail response: {}", e)))?;

        let mut discoveries = Vec::new();

        if let Some(messages) = list_response.messages {
            for message_ref in messages.iter().take(max_results as usize) {
                match self.fetch_message_details(&message_ref.id).await {
                    Ok(Some(discovery)) => discoveries.push(discovery),
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!("Failed to fetch message {}: {}", message_ref.id, e);
                    }
                }
            }
        }

        Ok(discoveries)
    }

    /// Fetch detailed message information
    async fn fetch_message_details(
        &self,
        message_id: &str,
    ) -> Result<Option<DiscoveredSubscription>, AppError> {
        let url = format!("{}/users/me/messages/{}", GMAIL_API_BASE, message_id);

        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[("format", "metadata")])
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to fetch message: {}", e)))?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let message: GmailMessage = response
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse message: {}", e)))?;

        // Extract "From" header
        if let Some(payload) = message.payload {
            if let Some(headers) = payload.headers {
                for header in headers {
                    if header.name.to_lowercase() == "from" {
                        let timestamp = message
                            .internal_date
                            .and_then(|s| s.parse::<i64>().ok())
                            .map(|ms| ms / 1000); // Convert ms to seconds

                        return Ok(self.parse_from_header(&header.value, timestamp));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Parse "From" header to extract sender info
    fn parse_from_header(
        &self,
        from: &str,
        timestamp: Option<i64>,
    ) -> Option<DiscoveredSubscription> {
        // Regex to parse email addresses from various formats
        // e.g., "Name <email@example.com>" or "email@example.com"
        let email_regex = Regex::new(r"<?([a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})>?").ok()?;
        let name_regex = Regex::new(r#"^"?([^"<]+)"?\s*<"#).ok()?;

        let email = email_regex
            .captures(from)?
            .get(1)?
            .as_str()
            .to_string();

        let sender_name = name_regex
            .captures(from)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string());

        // Extract domain
        let domain = email.split('@').nth(1)?.to_string();

        let now = chrono::Utc::now().timestamp();
        let seen_time = timestamp.unwrap_or(now);

        Some(DiscoveredSubscription {
            id: None,
            gmail_account_id: 0, // Will be set by caller
            sender_email: email,
            sender_name,
            domain,
            email_count: 1,
            first_seen_at: seen_time,
            last_seen_at: seen_time,
            status: DiscoveryStatus::Pending,
            linked_subscription_id: None,
            created_at: None,
        })
    }

    /// Deduplicate discoveries by sender email
    fn parse_and_deduplicate(
        &self,
        discoveries: Vec<DiscoveredSubscription>,
        config: &ScannerConfig,
    ) -> Vec<DiscoveredSubscription> {
        let mut map: HashMap<String, DiscoveredSubscription> = HashMap::new();

        for discovery in discoveries {
            // Check if domain is excluded
            if config.excluded_domains.contains(&discovery.domain) {
                continue;
            }

            map.entry(discovery.sender_email.clone())
                .and_modify(|existing| {
                    existing.email_count += 1;
                    existing.first_seen_at = existing.first_seen_at.min(discovery.first_seen_at);
                    existing.last_seen_at = existing.last_seen_at.max(discovery.last_seen_at);
                })
                .or_insert(discovery);
        }

        // Filter by minimum email threshold
        map.into_values()
            .filter(|d| d.email_count >= config.min_email_threshold as i32)
            .collect()
    }
}

// Helper module for URL encoding
mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}
