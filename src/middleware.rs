use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Local;
use std::time::Instant;

use crate::AppState;

pub async fn log_requests(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let start = Instant::now();
    let response = next.run(req).await;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let status = response.status().as_u16();
    let now = Local::now().format("%H:%M:%S");
    let color = match status {
        200..=299 => "\x1b[32m",
        300..=399 => "\x1b[36m",
        400..=499 => "\x1b[33m",
        _ => "\x1b[31m",
    };
    eprintln!("{now} {method:<5} {color}{status}\x1b[0m {elapsed_ms:>7.2}ms  {path}");
    response
}

/// Router fallback: render the themed 404 shell so unmatched URLs match the
/// look of a missing-repo page instead of axum's default plain-text body.
pub async fn not_found(State(state): State<AppState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let body = crate::render::render(
        &state,
        "not_found.html",
        &path,
        minijinja::context! { name => path.trim_start_matches('/') },
    );
    (StatusCode::NOT_FOUND, body).into_response()
}
