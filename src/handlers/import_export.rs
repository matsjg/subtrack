use crate::db::{self, DbPool};
use crate::error::AppError;
use crate::models::{BillingCycle, CsvSubscription};
use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::Response,
    Json,
};
use serde_json::json;
use std::io::Cursor;

pub async fn export_csv(State(pool): State<DbPool>) -> Result<Response, AppError> {
    let subscriptions = db::list_subscriptions(&pool)?;

    let mut wtr = csv::Writer::from_writer(vec![]);

    // Write header
    wtr.write_record(&[
        "name",
        "cost",
        "billing_cycle",
        "renewal_date",
        "category",
        "status",
        "notes",
        "reminder_days",
    ])?;

    // Write data
    for sub in subscriptions {
        wtr.write_record(&[
            sub.name,
            sub.cost.to_string(),
            sub.billing_cycle.to_str().to_string(),
            sub.renewal_date,
            sub.category.unwrap_or_default(),
            sub.status.to_str().to_string(),
            sub.notes.unwrap_or_default(),
            sub.reminder_days.to_string(),
        ])?;
    }

    let csv_data = wtr
        .into_inner()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/csv")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"subscriptions.csv\"",
        )
        .body(Body::from(csv_data))
        .unwrap())
}

pub async fn import_csv(
    State(pool): State<DbPool>,
    body: String,
) -> Result<Json<serde_json::Value>, AppError> {
    let cursor = Cursor::new(body);
    let mut rdr = csv::Reader::from_reader(cursor);

    let mut imported = 0;
    let mut errors = Vec::new();

    for (line_num, result) in rdr.deserialize::<CsvSubscription>().enumerate() {
        match result {
            Ok(record) => {
                // Validate and convert
                let billing_cycle = match BillingCycle::from_str(&record.billing_cycle) {
                    Ok(bc) => bc,
                    Err(e) => {
                        errors.push(format!("Line {}: {}", line_num + 2, e));
                        continue;
                    }
                };

                // Validate date
                if let Err(_) =
                    chrono::NaiveDate::parse_from_str(&record.renewal_date, "%Y-%m-%d")
                {
                    errors.push(format!(
                        "Line {}: Invalid date format (use YYYY-MM-DD)",
                        line_num + 2
                    ));
                    continue;
                }

                // Validate cost
                if record.cost < 0.0 {
                    errors.push(format!("Line {}: Cost cannot be negative", line_num + 2));
                    continue;
                }

                // Create subscription
                match db::create_subscription(
                    &pool,
                    &record.name,
                    record.cost,
                    &billing_cycle,
                    &record.renewal_date,
                    record.category.as_deref(),
                    record.notes.as_deref(),
                    record.reminder_days.unwrap_or(7),
                ) {
                    Ok(_) => imported += 1,
                    Err(e) => {
                        errors.push(format!("Line {}: Database error: {}", line_num + 2, e));
                    }
                }
            }
            Err(e) => {
                errors.push(format!("Line {}: CSV parse error: {}", line_num + 2, e));
            }
        }
    }

    Ok(Json(json!({
        "imported": imported,
        "errors": errors,
    })))
}
