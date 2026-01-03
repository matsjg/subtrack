mod config;
mod db;
mod error;
mod handlers;
mod models;

use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rust_embed::RustEmbed;
use tower_http::cors::CorsLayer;
use tracing_subscriber;

#[derive(RustEmbed)]
#[folder = "static/"]
struct StaticAssets;

async fn serve_static(path: &str) -> Response {
    let path = if path.is_empty() || path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    match StaticAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            // If file not found, try serving index.html for SPA routing
            if let Some(content) = StaticAssets::get("index.html") {
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(Body::from(content.data))
                    .unwrap()
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("Not found"))
                    .unwrap()
            }
        }
    }
}

async fn serve_index() -> Response {
    serve_static("index.html").await
}

async fn serve_file(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    serve_static(&path).await
}

async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

#[tokio::main]
async fn main() {
    // Load configuration
    let config = config::Config::load();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(&config.log_level)
        .init();

    tracing::info!("Starting SubTrack...");
    tracing::info!("Database: {}", config.database);

    // Initialize database
    let pool = db::initialize_db(&config.database).expect("Failed to initialize database");
    tracing::info!("Database initialized successfully");

    // Build API routes
    let api_routes = Router::new()
        .route("/subscriptions", get(handlers::subscriptions::list_subscriptions))
        .route("/subscriptions", post(handlers::subscriptions::create_subscription))
        .route("/subscriptions/:id", get(handlers::subscriptions::get_subscription))
        .route("/subscriptions/:id", axum::routing::put(handlers::subscriptions::update_subscription))
        .route("/subscriptions/:id", axum::routing::delete(handlers::subscriptions::delete_subscription))
        .route("/subscriptions/:id/cancel", post(handlers::subscriptions::cancel_subscription))
        .route("/subscriptions/:id/reactivate", post(handlers::subscriptions::reactivate_subscription))
        .route("/summary", get(handlers::summary::get_summary))
        .route("/export", get(handlers::import_export::export_csv))
        .route("/import", post(handlers::import_export::import_csv))
        .with_state(pool);

    // Build main app with static file serving
    let app = Router::new()
        .nest("/api", api_routes)
        .route("/health", get(health_check))
        .route("/", get(serve_index))
        .route("/*path", get(serve_file))
        .layer(CorsLayer::permissive());

    // Start server
    let bind_addr = config.bind_address();
    tracing::info!("Server listening on http://{}", bind_addr);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind to address");

    axum::serve(listener, app)
        .await
        .expect("Server failed to start");
}
