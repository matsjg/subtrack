use crate::error::AppError;
use crate::models::{DiscoveredSubscription, DiscoveryStatus, GmailAccount, ScanResult, ScannerConfig};
use google_gmail1::{api::Scope, Gmail};
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;
use regex::Regex;
use std::collections::HashMap;
use std::time::Instant;
use yup_oauth2::{InstalledFlowAuthenticator, InstalledFlowReturnMethod};

pub struct GmailScanner {
    hub: Gmail<HttpsConnector<HttpConnector>>,
    account_email: String,
}

impl GmailScanner {
    /// Create a new Gmail scanner with OAuth authentication
    pub async fn new(
        client_id: &str,
        client_secret: &str,
        account_email: &str,
    ) -> Result<(Self, GmailAccount), AppError> {
        // Create OAuth2 authenticator
        let secret = yup_oauth2::ApplicationSecret {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            ..Default::default()
        };

        let auth = InstalledFlowAuthenticator::builder(
            secret,
            InstalledFlowReturnMethod::HTTPRedirect,
        )
        .persist_tokens_to_disk("token_cache.json")
        .build()
        .await
        .map_err(|e| AppError::Internal(format!("OAuth setup failed: {}", e)))?;

        // Request Gmail read-only scope
        let scopes = &[Scope::Readonly];
        let token = auth
            .token(scopes)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to get OAuth token: {}", e)))?;

        // Create Gmail API client
        let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_webpki_roots()
                    .https_or_http()
                    .enable_http1()
                    .build(),
            );

        let hub = Gmail::new(client, auth);

        // Extract token details for storage
        let gmail_account = GmailAccount {
            id: None,
            email: account_email.to_string(),
            access_token: token.token().unwrap_or_default().to_string(),
            refresh_token: String::new(), // Managed by yup-oauth2
            token_expiry: token
                .expiration_time()
                .map(|t| t.timestamp())
                .unwrap_or(0),
            created_at: None,
            last_scan_at: None,
        };

        Ok((
            Self {
                hub,
                account_email: account_email.to_string(),
            },
            gmail_account,
        ))
    }

    /// Scan Gmail inbox for subscription-related emails
    pub async fn scan(&self, config: &ScannerConfig) -> Result<ScanResult, AppError> {
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

        Ok(ScanResult {
            total_emails_scanned: total_emails,
            new_discoveries: discoveries.len() as u32,
            updated_discoveries: 0, // Will be set by database layer
            scan_duration_ms: scan_duration.as_millis() as u64,
        })
    }

    /// Fetch messages matching a query
    async fn fetch_messages(
        &self,
        query: &str,
        max_results: u32,
    ) -> Result<Vec<DiscoveredSubscription>, AppError> {
        let result = self
            .hub
            .users()
            .messages_list("me")
            .q(query)
            .max_results(max_results)
            .doit()
            .await
            .map_err(|e| AppError::Internal(format!("Gmail API error: {}", e)))?;

        let mut discoveries = Vec::new();

        if let Some(messages) = result.1.messages {
            for message_ref in messages.iter().take(max_results as usize) {
                if let Some(id) = &message_ref.id {
                    match self.fetch_message_details(id).await {
                        Ok(Some(discovery)) => discoveries.push(discovery),
                        Ok(None) => {}
                        Err(e) => {
                            tracing::warn!("Failed to fetch message {}: {}", id, e);
                        }
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
        let result = self
            .hub
            .users()
            .messages_get("me", message_id)
            .format("metadata")
            .doit()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to get message: {}", e)))?;

        let message = result.1;

        // Extract "From" header
        if let Some(payload) = message.payload {
            if let Some(headers) = payload.headers {
                for header in headers {
                    if header.name.as_deref() == Some("From") {
                        if let Some(from_value) = header.value {
                            return Ok(self.parse_from_header(&from_value, message.internal_date));
                        }
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
        let seen_time = timestamp.unwrap_or(now) / 1000; // Convert ms to seconds

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
