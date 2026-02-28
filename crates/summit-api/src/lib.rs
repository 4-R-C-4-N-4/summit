pub mod handlers;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};

pub use handlers::ApiState;

pub async fn serve(state: ApiState, port: u16) -> anyhow::Result<()> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_routes = Router::new()
        .route("/status", get(handlers::handle_status))
        .route("/peers", get(handlers::handle_peers))
        .route("/cache", get(handlers::handle_cache))
        .route("/cache/clear", post(handlers::handle_cache_clear))
        .route(
            "/send",
            post(handlers::handle_send).layer(DefaultBodyLimit::max(256 * 1024 * 1024)),
        )
        .route("/files", get(handlers::handle_files))
        .route("/trust", get(handlers::handle_trust_list))
        .route("/trust/add", post(handlers::handle_trust_add))
        .route("/trust/block", post(handlers::handle_trust_block))
        .route("/trust/pending", get(handlers::handle_trust_pending))
        .route("/daemon/shutdown", post(handlers::handle_shutdown))
        .route("/sessions/{id}", delete(handlers::handle_session_drop))
        .route("/sessions/{id}", get(handlers::handle_session_inspect))
        .route("/schema", get(handlers::handle_schema_list))
        .route(
            "/messages/{peer_pubkey}",
            get(handlers::handle_get_messages),
        )
        .route("/messages/send", post(handlers::handle_send_message))
        .route("/services", get(handlers::handle_services))
        .route("/compute/tasks", get(handlers::handle_compute_all_tasks))
        .route(
            "/compute/tasks/{peer_pubkey}",
            get(handlers::handle_compute_tasks),
        )
        .route("/compute/submit", post(handlers::handle_compute_submit))
        .with_state(state);

    let app = Router::new().nest("/api", api_routes).layer(cors);

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    tracing::info!(port, "API listening on 127.0.0.1");
    axum::serve(listener, app).await?;
    Ok(())
}
