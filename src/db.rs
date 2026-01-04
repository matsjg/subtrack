use crate::error::AppError;
use crate::models::{
    BillingCycle, CategorySummary, DiscoveredSubscription, DiscoveryStatus, GmailAccount,
    Subscription, SubscriptionStatus, Summary, UpcomingRenewal,
};
use chrono::{NaiveDate, Utc};
use rusqlite::{params, Connection, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type DbPool = Arc<Mutex<Connection>>;

pub fn initialize_db(db_path: &str) -> Result<DbPool> {
    let conn = Connection::open(db_path)?;
    run_migrations(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS subscriptions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            cost REAL NOT NULL,
            billing_cycle TEXT NOT NULL CHECK(billing_cycle IN ('weekly', 'monthly', 'quarterly', 'yearly')),
            renewal_date TEXT NOT NULL,
            category TEXT,
            status TEXT DEFAULT 'active' CHECK(status IN ('active', 'paused', 'canceled')),
            notes TEXT,
            reminder_days INTEGER DEFAULT 7,
            created_at TEXT DEFAULT (datetime('now')),
            updated_at TEXT DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions(status);
        CREATE INDEX IF NOT EXISTS idx_subscriptions_renewal ON subscriptions(renewal_date);
        CREATE INDEX IF NOT EXISTS idx_subscriptions_category ON subscriptions(category);

        -- Gmail integration tables
        CREATE TABLE IF NOT EXISTS gmail_accounts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL UNIQUE,
            access_token TEXT NOT NULL,
            refresh_token TEXT NOT NULL,
            token_expiry INTEGER NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            last_scan_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS discovered_subscriptions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            gmail_account_id INTEGER NOT NULL,
            sender_email TEXT NOT NULL,
            sender_name TEXT,
            domain TEXT NOT NULL,
            email_count INTEGER DEFAULT 1,
            first_seen_at INTEGER NOT NULL,
            last_seen_at INTEGER NOT NULL,
            status TEXT DEFAULT 'pending' CHECK (status IN ('pending', 'accepted', 'ignored')),
            linked_subscription_id INTEGER,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            FOREIGN KEY (gmail_account_id) REFERENCES gmail_accounts(id) ON DELETE CASCADE,
            FOREIGN KEY (linked_subscription_id) REFERENCES subscriptions(id) ON DELETE SET NULL,
            UNIQUE (gmail_account_id, sender_email)
        );

        CREATE INDEX IF NOT EXISTS idx_discovered_status ON discovered_subscriptions(status);
        CREATE INDEX IF NOT EXISTS idx_discovered_domain ON discovered_subscriptions(domain);
        CREATE INDEX IF NOT EXISTS idx_discovered_gmail_account ON discovered_subscriptions(gmail_account_id);
        "#,
    )?;
    Ok(())
}

pub fn create_subscription(
    pool: &DbPool,
    name: &str,
    cost: f64,
    billing_cycle: &BillingCycle,
    renewal_date: &str,
    category: Option<&str>,
    notes: Option<&str>,
    reminder_days: i32,
) -> Result<i64, AppError> {
    let conn = pool.lock().unwrap();
    conn.execute(
        "INSERT INTO subscriptions (name, cost, billing_cycle, renewal_date, category, notes, reminder_days)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            name,
            cost,
            billing_cycle.to_str(),
            renewal_date,
            category,
            notes,
            reminder_days
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_subscription(pool: &DbPool, id: i64) -> Result<Option<Subscription>, AppError> {
    let conn = pool.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, name, cost, billing_cycle, renewal_date, category, status, notes, reminder_days, created_at, updated_at
         FROM subscriptions WHERE id = ?1",
    )?;

    let result = stmt.query_row(params![id], |row| {
        Ok(Subscription {
            id: Some(row.get(0)?),
            name: row.get(1)?,
            cost: row.get(2)?,
            billing_cycle: BillingCycle::from_str(&row.get::<_, String>(3)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            renewal_date: row.get(4)?,
            category: row.get(5)?,
            status: SubscriptionStatus::from_str(&row.get::<_, String>(6)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            notes: row.get(7)?,
            reminder_days: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    });

    match result {
        Ok(sub) => Ok(Some(sub)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e)),
    }
}

pub fn list_subscriptions(pool: &DbPool) -> Result<Vec<Subscription>, AppError> {
    let conn = pool.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, name, cost, billing_cycle, renewal_date, category, status, notes, reminder_days, created_at, updated_at
         FROM subscriptions ORDER BY renewal_date ASC",
    )?;

    let subscriptions = stmt
        .query_map([], |row| {
            Ok(Subscription {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                cost: row.get(2)?,
                billing_cycle: BillingCycle::from_str(&row.get::<_, String>(3)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                renewal_date: row.get(4)?,
                category: row.get(5)?,
                status: SubscriptionStatus::from_str(&row.get::<_, String>(6)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                notes: row.get(7)?,
                reminder_days: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(subscriptions)
}

pub fn update_subscription(
    pool: &DbPool,
    id: i64,
    name: Option<&str>,
    cost: Option<f64>,
    billing_cycle: Option<&BillingCycle>,
    renewal_date: Option<&str>,
    category: Option<Option<&str>>,
    notes: Option<Option<&str>>,
    reminder_days: Option<i32>,
    status: Option<&SubscriptionStatus>,
) -> Result<bool, AppError> {
    let conn = pool.lock().unwrap();

    // Build dynamic update query
    let mut updates = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(n) = name {
        updates.push("name = ?");
        params_vec.push(Box::new(n.to_string()));
    }
    if let Some(c) = cost {
        updates.push("cost = ?");
        params_vec.push(Box::new(c));
    }
    if let Some(bc) = billing_cycle {
        updates.push("billing_cycle = ?");
        params_vec.push(Box::new(bc.to_str().to_string()));
    }
    if let Some(rd) = renewal_date {
        updates.push("renewal_date = ?");
        params_vec.push(Box::new(rd.to_string()));
    }
    if let Some(cat) = category {
        updates.push("category = ?");
        params_vec.push(Box::new(cat.map(|s| s.to_string())));
    }
    if let Some(n) = notes {
        updates.push("notes = ?");
        params_vec.push(Box::new(n.map(|s| s.to_string())));
    }
    if let Some(rd) = reminder_days {
        updates.push("reminder_days = ?");
        params_vec.push(Box::new(rd));
    }
    if let Some(s) = status {
        updates.push("status = ?");
        params_vec.push(Box::new(s.to_str().to_string()));
    }

    if updates.is_empty() {
        return Ok(false);
    }

    updates.push("updated_at = datetime('now')");

    let query = format!(
        "UPDATE subscriptions SET {} WHERE id = ?",
        updates.join(", ")
    );

    params_vec.push(Box::new(id));

    let params_refs: Vec<&dyn rusqlite::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();

    let rows_affected = conn.execute(&query, params_refs.as_slice())?;

    Ok(rows_affected > 0)
}

pub fn delete_subscription(pool: &DbPool, id: i64) -> Result<bool, AppError> {
    let conn = pool.lock().unwrap();
    let rows_affected = conn.execute("DELETE FROM subscriptions WHERE id = ?1", params![id])?;
    Ok(rows_affected > 0)
}

pub fn cancel_subscription(pool: &DbPool, id: i64) -> Result<bool, AppError> {
    let conn = pool.lock().unwrap();
    let rows_affected = conn.execute(
        "UPDATE subscriptions SET status = 'canceled', updated_at = datetime('now') WHERE id = ?1",
        params![id],
    )?;
    Ok(rows_affected > 0)
}

pub fn reactivate_subscription(pool: &DbPool, id: i64) -> Result<bool, AppError> {
    let conn = pool.lock().unwrap();
    let rows_affected = conn.execute(
        "UPDATE subscriptions SET status = 'active', updated_at = datetime('now') WHERE id = ?1",
        params![id],
    )?;
    Ok(rows_affected > 0)
}

pub fn get_summary(pool: &DbPool) -> Result<Summary, AppError> {
    let subscriptions = list_subscriptions(pool)?;

    let active_subs: Vec<_> = subscriptions
        .iter()
        .filter(|s| matches!(s.status, SubscriptionStatus::Active))
        .collect();

    let mut total_monthly = 0.0;
    let mut category_map: HashMap<String, (f64, i32)> = HashMap::new();

    for sub in &active_subs {
        let monthly_cost = sub.cost / sub.billing_cycle.months();
        total_monthly += monthly_cost;

        if let Some(ref cat) = sub.category {
            let entry = category_map.entry(cat.clone()).or_insert((0.0, 0));
            entry.0 += monthly_cost;
            entry.1 += 1;
        }
    }

    let total_yearly = total_monthly * 12.0;

    let categories: HashMap<String, CategorySummary> = category_map
        .into_iter()
        .map(|(cat, (monthly, count))| {
            (
                cat,
                CategorySummary {
                    monthly,
                    yearly: monthly * 12.0,
                    count,
                },
            )
        })
        .collect();

    // Get upcoming renewals (next 30 days)
    let today = Utc::now().naive_utc().date();
    let upcoming_renewals = active_subs
        .iter()
        .filter_map(|sub| {
            let renewal = NaiveDate::parse_from_str(&sub.renewal_date, "%Y-%m-%d").ok()?;
            let days_until = (renewal - today).num_days();
            if days_until >= 0 && days_until <= 30 {
                Some(UpcomingRenewal {
                    id: sub.id?,
                    name: sub.name.clone(),
                    cost: sub.cost,
                    next_renewal: sub.renewal_date.clone(),
                    days_until,
                })
            } else {
                None
            }
        })
        .collect();

    Ok(Summary {
        total_monthly,
        total_yearly,
        active_count: active_subs.len() as i32,
        categories,
        upcoming_renewals,
    })
}

// Gmail account operations
pub fn save_gmail_account(pool: &DbPool, account: &GmailAccount) -> Result<i64, AppError> {
    let conn = pool.lock().unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO gmail_accounts (email, access_token, refresh_token, token_expiry)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            account.email,
            account.access_token,
            account.refresh_token,
            account.token_expiry
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_gmail_account(pool: &DbPool, email: &str) -> Result<Option<GmailAccount>, AppError> {
    let conn = pool.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, email, access_token, refresh_token, token_expiry, created_at, last_scan_at
         FROM gmail_accounts WHERE email = ?1",
    )?;

    let result = stmt.query_row(params![email], |row| {
        Ok(GmailAccount {
            id: Some(row.get(0)?),
            email: row.get(1)?,
            access_token: row.get(2)?,
            refresh_token: row.get(3)?,
            token_expiry: row.get(4)?,
            created_at: row.get(5)?,
            last_scan_at: row.get(6)?,
        })
    });

    match result {
        Ok(account) => Ok(Some(account)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e)),
    }
}

pub fn list_gmail_accounts(pool: &DbPool) -> Result<Vec<GmailAccount>, AppError> {
    let conn = pool.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, email, access_token, refresh_token, token_expiry, created_at, last_scan_at
         FROM gmail_accounts ORDER BY created_at DESC",
    )?;

    let accounts = stmt
        .query_map([], |row| {
            Ok(GmailAccount {
                id: Some(row.get(0)?),
                email: row.get(1)?,
                access_token: row.get(2)?,
                refresh_token: row.get(3)?,
                token_expiry: row.get(4)?,
                created_at: row.get(5)?,
                last_scan_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(accounts)
}

pub fn update_gmail_scan_time(pool: &DbPool, account_id: i64) -> Result<(), AppError> {
    let conn = pool.lock().unwrap();
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE gmail_accounts SET last_scan_at = ?1 WHERE id = ?2",
        params![now, account_id],
    )?;
    Ok(())
}

pub fn delete_gmail_account(pool: &DbPool, account_id: i64) -> Result<bool, AppError> {
    let conn = pool.lock().unwrap();
    let rows_affected = conn.execute("DELETE FROM gmail_accounts WHERE id = ?1", params![account_id])?;
    Ok(rows_affected > 0)
}

// Discovered subscriptions operations
pub fn upsert_discovered_subscription(
    pool: &DbPool,
    discovery: &DiscoveredSubscription,
) -> Result<i64, AppError> {
    let conn = pool.lock().unwrap();

    // Try to insert or update
    conn.execute(
        "INSERT INTO discovered_subscriptions
         (gmail_account_id, sender_email, sender_name, domain, email_count, first_seen_at, last_seen_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(gmail_account_id, sender_email) DO UPDATE SET
         email_count = email_count + ?5,
         last_seen_at = ?7,
         sender_name = COALESCE(?3, sender_name)",
        params![
            discovery.gmail_account_id,
            discovery.sender_email,
            discovery.sender_name,
            discovery.domain,
            discovery.email_count,
            discovery.first_seen_at,
            discovery.last_seen_at,
            discovery.status.to_str()
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

pub fn list_discovered_subscriptions(
    pool: &DbPool,
    gmail_account_id: i64,
    status_filter: Option<DiscoveryStatus>,
) -> Result<Vec<DiscoveredSubscription>, AppError> {
    let conn = pool.lock().unwrap();

    let query = if status_filter.is_some() {
        "SELECT id, gmail_account_id, sender_email, sender_name, domain, email_count,
                first_seen_at, last_seen_at, status, linked_subscription_id, created_at
         FROM discovered_subscriptions
         WHERE gmail_account_id = ?1 AND status = ?2
         ORDER BY email_count DESC, last_seen_at DESC"
    } else {
        "SELECT id, gmail_account_id, sender_email, sender_name, domain, email_count,
                first_seen_at, last_seen_at, status, linked_subscription_id, created_at
         FROM discovered_subscriptions
         WHERE gmail_account_id = ?1
         ORDER BY email_count DESC, last_seen_at DESC"
    };

    let mut stmt = conn.prepare(query)?;

    let discoveries = if let Some(status) = status_filter {
        stmt.query_map(params![gmail_account_id, status.to_str()], |row| {
            Ok(DiscoveredSubscription {
                id: Some(row.get(0)?),
                gmail_account_id: row.get(1)?,
                sender_email: row.get(2)?,
                sender_name: row.get(3)?,
                domain: row.get(4)?,
                email_count: row.get(5)?,
                first_seen_at: row.get(6)?,
                last_seen_at: row.get(7)?,
                status: DiscoveryStatus::from_str(&row.get::<_, String>(8)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                linked_subscription_id: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![gmail_account_id], |row| {
            Ok(DiscoveredSubscription {
                id: Some(row.get(0)?),
                gmail_account_id: row.get(1)?,
                sender_email: row.get(2)?,
                sender_name: row.get(3)?,
                domain: row.get(4)?,
                email_count: row.get(5)?,
                first_seen_at: row.get(6)?,
                last_seen_at: row.get(7)?,
                status: DiscoveryStatus::from_str(&row.get::<_, String>(8)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                linked_subscription_id: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
    };

    Ok(discoveries)
}

pub fn update_discovery_status(
    pool: &DbPool,
    discovery_id: i64,
    status: DiscoveryStatus,
    linked_subscription_id: Option<i64>,
) -> Result<bool, AppError> {
    let conn = pool.lock().unwrap();
    let rows_affected = conn.execute(
        "UPDATE discovered_subscriptions SET status = ?1, linked_subscription_id = ?2 WHERE id = ?3",
        params![status.to_str(), linked_subscription_id, discovery_id],
    )?;
    Ok(rows_affected > 0)
}

pub fn delete_discovered_subscription(pool: &DbPool, discovery_id: i64) -> Result<bool, AppError> {
    let conn = pool.lock().unwrap();
    let rows_affected = conn.execute(
        "DELETE FROM discovered_subscriptions WHERE id = ?1",
        params![discovery_id],
    )?;
    Ok(rows_affected > 0)
}
