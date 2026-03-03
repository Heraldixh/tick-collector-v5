//! Tick Collector Web Dashboard
//! 
//! A lightweight web server for the Tick Collector trading platform.
//! Designed to run on GCP Free Tier (f1-micro, 0.6GB RAM).
//!
//! Features:
//! - REST API for historical data
//! - WebSocket streaming for real-time ticks
//! - Static file serving for HTML/JS frontend
//! - Multi-chart support (up to 9 charts)
//! - Background data persistence with 7-day retention
//! - Admin authentication for browser access
//! - API key authentication for desktop clients

use actix_web::{web, App, HttpServer, middleware};
use actix_files::Files;
use std::sync::Arc;
use std::path::PathBuf;
use parking_lot::RwLock;

mod api;
mod websocket;
mod exchange;
mod state;

use state::AppState;

const VERSION: &str = "1.0.0";

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize logging
    if let Err(e) = setup_logging() {
        eprintln!("Failed to setup logging: {}", e);
        std::process::exit(1);
    }
    
    // Initialize start time for uptime tracking
    api::init_start_time();
    
    // Create data directories
    std::fs::create_dir_all("data/trades").ok();
    std::fs::create_dir_all("data/footprint").ok();
    
    log::info!("═══════════════════════════════════════════════════════════");
    log::info!("🚀 Tick Collector Web v{} - Production Server", VERSION);
    log::info!("═══════════════════════════════════════════════════════════");
    log::info!("📊 Dashboard: http://0.0.0.0:8080");
    log::info!("🔧 API: http://0.0.0.0:8080/api/v1");
    log::info!("💚 Health: http://0.0.0.0:8080/api/v1/health");
    log::info!("📦 Data retention: 7 days");
    log::info!("💾 Persistence interval: 30 seconds");
    log::info!("═══════════════════════════════════════════════════════════");
    
    // Get static files path
    let static_path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "static"].iter().collect();
    log::info!("📁 Static files: {:?}", static_path);
    
    // Initialize shared state
    let app_state = Arc::new(RwLock::new(AppState::new()));
    
    // Start exchange WebSocket connections in background
    let state_clone = Arc::clone(&app_state);
    tokio::spawn(async move {
        exchange::start_exchange_connections(state_clone).await;
    });
    
    // Start background data persistence task (saves trades to files every 30 seconds)
    let state_for_persistence = Arc::clone(&app_state);
    tokio::spawn(async move {
        api::start_background_persistence(state_for_persistence).await;
    });
    
    // Start HTTP server
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&app_state)))
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            // API routes
            .service(
                web::scope("/api/v1")
                    .route("/health", web::get().to(api::health))
                    .route("/tickers", web::get().to(api::get_tickers))
                    .route("/tickers/{exchange}/{symbol}/candles", web::get().to(api::get_candles))
                    .route("/tickers/{exchange}/{symbol}/trades", web::get().to(api::get_trades))
                    .route("/config", web::get().to(api::get_config))
                    .route("/config", web::post().to(api::save_config))
                    // Health monitoring
                    .route("/health/detailed", web::get().to(api::health_detailed))
                    // Footprint data persistence (server-side storage)
                    .route("/footprint/{exchange}/{symbol}", web::get().to(api::get_footprint))
                    .route("/footprint/{exchange}/{symbol}", web::post().to(api::save_footprint))
                    // Backup trades from exchange REST API (for WebSocket gaps)
                    .route("/backup-trades/{exchange}/{symbol}", web::get().to(api::get_backup_trades))
                    // Authentication endpoints
                    .route("/auth/status", web::get().to(api::auth_status))
                    .route("/auth/setup", web::post().to(api::auth_setup))
                    .route("/auth/login", web::post().to(api::auth_login))
                    .route("/auth/logout", web::post().to(api::auth_logout))
                    .route("/auth/update", web::post().to(api::auth_update))
                    .route("/auth/check", web::get().to(api::auth_check))
                    // API Key endpoints (for web app to display and regenerate)
                    .route("/api-key", web::get().to(api::get_api_key))
                    .route("/api-key/regenerate", web::post().to(api::regenerate_api_key))
                    // Sync API for desktop client (requires API Key)
                    .route("/sync/tickers", web::get().to(api::sync_tickers))
                    .route("/sync/{exchange}/{symbol}/latest", web::get().to(api::sync_latest))
                    .route("/sync/{exchange}/{symbol}/trades", web::get().to(api::sync_trades))
                    .route("/sync/{exchange}/{symbol}/bars", web::get().to(api::sync_bars))
                    // Storage management (admin only)
                    .route("/storage/list", web::get().to(api::list_storage))
                    .route("/storage/clear-all", web::delete().to(api::clear_all_storage))
                    .route("/storage/clear/{exchange}/{symbol}", web::delete().to(api::clear_ticker_storage))
            )
            // WebSocket for live data
            .route("/ws/live/{exchange}/{symbol}", web::get().to(websocket::ws_handler))
            // Static files (HTML/JS/CSS)
            .service(Files::new("/", static_path.clone()).index_file("index.html"))
    })
    .bind("0.0.0.0:8080")?
    .workers(1) // Single worker for f1-micro (0.6GB RAM)
    .shutdown_timeout(30) // 30 second graceful shutdown
    .keep_alive(std::time::Duration::from_secs(75)) // Keep-alive for WebSocket connections
    .client_request_timeout(std::time::Duration::from_secs(60))
    .run()
    .await
}

fn setup_logging() -> Result<(), fern::InitError> {
    // Reduce log verbosity for production
    let log_level = std::env::var("RUST_LOG")
        .map(|l| match l.to_lowercase().as_str() {
            "debug" => log::LevelFilter::Debug,
            "warn" => log::LevelFilter::Warn,
            "error" => log::LevelFilter::Error,
            _ => log::LevelFilter::Info,
        })
        .unwrap_or(log::LevelFilter::Info);
    
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(log_level)
        // Reduce noise from dependencies
        .level_for("actix_web", log::LevelFilter::Warn)
        .level_for("actix_http", log::LevelFilter::Warn)
        .level_for("actix_server", log::LevelFilter::Warn)
        .level_for("mio", log::LevelFilter::Warn)
        .level_for("tokio_tungstenite", log::LevelFilter::Warn)
        .level_for("tungstenite", log::LevelFilter::Warn)
        .chain(std::io::stdout())
        .apply()?;
    
    Ok(())
}
