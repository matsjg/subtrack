use crate::error::AppError;
use crate::models::{
    BillingCycle, CategorySummary, Subscription, SubscriptionStatus, Summary, UpcomingRenewal,
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
