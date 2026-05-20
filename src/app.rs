use axum::{http::header, middleware as axum_middleware, Router};
use minijinja::Environment;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::middleware::log_requests;
use crate::routes;
use crate::templates;

#[derive(Clone)]
pub struct AppState {
    pub env: Arc<Environment<'static>>,
    pub config: Arc<Config>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub root: PathBuf,
    pub repo_root: PathBuf,
    pub base_url: String,
    pub clone_base: String,
    pub site_title: String,
    pub site_tagline: String,
}

impl AppState {
    pub fn from_env() -> Self {
        let root: PathBuf = std::env::var("REPOS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let repo_root: PathBuf = std::env::var("REPOS_REPO_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/srv/git"));
        let base_url = std::env::var("BASE_URL").unwrap_or_default();
        let clone_base = std::env::var("REPOS_CLONE_BASE")
            .unwrap_or_else(|_| "https://repos.bythewood.me".to_string());
        let site_title = std::env::var("REPOS_TITLE").unwrap_or_else(|_| "repos".to_string());
        let site_tagline = std::env::var("REPOS_TAGLINE").unwrap_or_default();

        let templates_dir = root.join("templates");
        let manifest_path = root.join("dist/.vite/manifest.json");
        let env = Arc::new(templates::build_env(&templates_dir, &manifest_path));

        let config = Arc::new(Config {
            root,
            repo_root,
            base_url,
            clone_base,
            site_title,
            site_tagline,
        });

        Self { env, config }
    }
}

pub fn router(state: AppState) -> Router {
    let dist_dir = state.config.root.join("dist");

    let static_cache = SetResponseHeaderLayer::if_not_present(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=31536000, immutable"),
    );

    Router::new()
        // Smart-HTTP clone endpoints live at /:name.git/... and need to be
        // matched before the catch-all repo routes; merge clone first.
        .merge(routes::clone::router())
        // seo routes (favicon, robots) before the repo catch-all so
        // /favicon.ico doesn't hit /:name.
        .merge(routes::seo::router())
        .merge(routes::index::router())
        .merge(routes::atom::router())
        .merge(routes::log::router())
        .merge(routes::commit::router())
        .merge(routes::tree::router())
        .merge(routes::blob::router())
        // repo landing page is the most general single-segment route, so
        // merge it last to avoid swallowing /static, /favicon.ico, etc.
        .merge(routes::repo::router())
        .nest_service(
            "/static",
            tower::ServiceBuilder::new()
                .layer(static_cache)
                .service(ServeDir::new(&dist_dir)),
        )
        .fallback(crate::middleware::not_found)
        .layer(axum_middleware::from_fn(log_requests))
        .with_state(state)
}
