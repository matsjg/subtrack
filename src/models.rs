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
