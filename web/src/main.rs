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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize logging
    setup_logging().expect("Failed to setup logging");
    
    log::info!("🚀 Tick Collector Web v0.1.0 starting...");
    log::info!("📊 Dashboard: http://localhost:8080");
    log::info!("🔧 API: http://localhost:8080/api/v1");
    
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
            )
            // WebSocket for live data
            .route("/ws/live/{exchange}/{symbol}", web::get().to(websocket::ws_handler))
            // Static files (HTML/JS/CSS)
            .service(Files::new("/", static_path.clone()).index_file("index.html"))
    })
    .bind("0.0.0.0:8080")?
    .workers(1) // Single worker for f1-micro (0.6GB RAM)
    .run()
    .await
}

fn setup_logging() -> Result<(), fern::InitError> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply()?;
    
    Ok(())
}
