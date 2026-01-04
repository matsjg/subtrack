use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: Option<i64>,
    pub name: String,
    pub cost: f64,
    pub billing_cycle: BillingCycle,
    pub renewal_date: String, // ISO 8601 date (YYYY-MM-DD)
    pub category: Option<String>,
    pub status: SubscriptionStatus,
    pub notes: Option<String>,
    pub reminder_days: i32,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BillingCycle {
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl BillingCycle {
    pub fn to_str(&self) -> &str {
        match self {
            BillingCycle::Weekly => "weekly",
            BillingCycle::Monthly => "monthly",
            BillingCycle::Quarterly => "quarterly",
            BillingCycle::Yearly => "yearly",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "weekly" => Ok(BillingCycle::Weekly),
            "monthly" => Ok(BillingCycle::Monthly),
            "quarterly" => Ok(BillingCycle::Quarterly),
            "yearly" => Ok(BillingCycle::Yearly),
            _ => Err(format!("Invalid billing cycle: {}", s)),
        }
    }

    pub fn months(&self) -> f64 {
        match self {
            BillingCycle::Weekly => 1.0 / 4.33,      // ~0.23 months
            BillingCycle::Monthly => 1.0,
            BillingCycle::Quarterly => 3.0,
            BillingCycle::Yearly => 12.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionStatus {
    Active,
    Paused,
    Canceled,
}

impl SubscriptionStatus {
    pub fn to_str(&self) -> &str {
        match self {
            SubscriptionStatus::Active => "active",
            SubscriptionStatus::Paused => "paused",
            SubscriptionStatus::Canceled => "canceled",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "active" => Ok(SubscriptionStatus::Active),
            "paused" => Ok(SubscriptionStatus::Paused),
            "canceled" => Ok(SubscriptionStatus::Canceled),
            _ => Err(format!("Invalid status: {}", s)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSubscriptionRequest {
    pub name: String,
    pub cost: f64,
    pub billing_cycle: BillingCycle,
    pub renewal_date: String,
    pub category: Option<String>,
    pub notes: Option<String>,
    pub reminder_days: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSubscriptionRequest {
    pub name: Option<String>,
    pub cost: Option<f64>,
    pub billing_cycle: Option<BillingCycle>,
    pub renewal_date: Option<String>,
    pub category: Option<String>,
    pub notes: Option<String>,
    pub reminder_days: Option<i32>,
    pub status: Option<SubscriptionStatus>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionResponse {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpcomingRenewal {
    pub id: i64,
    pub name: String,
    pub cost: f64,
    pub next_renewal: String,
    pub days_until: i64,
}

#[derive(Debug, Serialize)]
pub struct CategorySummary {
    pub monthly: f64,
    pub yearly: f64,
    pub count: i32,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub total_monthly: f64,
    pub total_yearly: f64,
    pub active_count: i32,
    pub categories: std::collections::HashMap<String, CategorySummary>,
    pub upcoming_renewals: Vec<UpcomingRenewal>,
}

#[derive(Debug, Deserialize)]
pub struct CsvSubscription {
    pub name: String,
    pub cost: f64,
    pub billing_cycle: String,
    pub renewal_date: String,
    pub category: Option<String>,
    #[allow(dead_code)]
    pub status: Option<String>,
    pub notes: Option<String>,
    pub reminder_days: Option<i32>,
}

// Gmail integration models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailAccount {
    pub id: Option<i64>,
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    pub token_expiry: i64,
    pub created_at: Option<i64>,
    pub last_scan_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscoveryStatus {
    Pending,
    Accepted,
    Ignored,
}

impl DiscoveryStatus {
    pub fn to_str(&self) -> &str {
        match self {
            DiscoveryStatus::Pending => "pending",
            DiscoveryStatus::Accepted => "accepted",
            DiscoveryStatus::Ignored => "ignored",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "pending" => Ok(DiscoveryStatus::Pending),
            "accepted" => Ok(DiscoveryStatus::Accepted),
            "ignored" => Ok(DiscoveryStatus::Ignored),
            _ => Err(format!("Invalid discovery status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredSubscription {
    pub id: Option<i64>,
    pub gmail_account_id: i64,
    pub sender_email: String,
    pub sender_name: Option<String>,
    pub domain: String,
    pub email_count: i32,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub status: DiscoveryStatus,
    pub linked_subscription_id: Option<i64>,
    pub created_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub total_emails_scanned: u32,
    pub new_discoveries: u32,
    pub updated_discoveries: u32,
    pub scan_duration_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct ScannerConfig {
    pub max_results_per_query: u32,
    pub lookback_days: u32,
    pub min_email_threshold: u32,
    pub custom_queries: Vec<String>,
    pub excluded_domains: Vec<String>,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            max_results_per_query: 500,
            lookback_days: 365,
            min_email_threshold: 2,
            custom_queries: Vec::new(),
            excluded_domains: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AcceptDiscoveryRequest {
    pub name: String,
    pub cost: f64,
    pub billing_cycle: BillingCycle,
    pub renewal_date: String,
    pub category: Option<String>,
    pub notes: Option<String>,
    pub reminder_days: Option<i32>,
}
