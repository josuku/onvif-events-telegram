use crate::{
    auth_middleware::auth_middleware,
    dto::*,
    routes::{auth_routes::*, camera_routes::*, config_routes::*, media_routes::*},
    state::AppState,
};
use app_core::domain::{camera::*, config::*, object::*};
use axum::{Router, response::IntoResponse};
use tower_http::cors::{Any, CorsLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(rust_embed::Embed)]
#[folder = "web/"]
pub struct WebAssets;

pub async fn static_handler(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    let mut path = uri.path().trim_start_matches('/').to_string();
    if path.is_empty() {
        path = "index.html".to_string();
    }

    if let Some(content) = WebAssets::get(&path) {
        let mime = mime_guess::from_path(&path).first_or_octet_stream();
        return (
            [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
            axum::body::Body::from(content.data),
        )
            .into_response();
    }

    // SPA fallback: return index.html for frontend navigation routes
    if !path.contains('.') && !path.starts_with("api/") && !path.starts_with("swagger-ui") {
        if let Some(content) = WebAssets::get("index.html") {
            return (
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                axum::body::Body::from(content.data),
            )
                .into_response();
        }
    }

    (axum::http::StatusCode::NOT_FOUND, "Not Found").into_response()
}

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                utoipa::openapi::security::SecurityScheme::Http(
                    utoipa::openapi::security::HttpBuilder::new()
                        .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::auth_routes::login,
        crate::routes::auth_routes::logout,
        crate::routes::auth_routes::current_user,
        crate::routes::camera_routes::get_cameras,
        crate::routes::camera_routes::get_camera,
        crate::routes::camera_routes::add_camera,
        crate::routes::camera_routes::delete_camera,
        crate::routes::camera_routes::set_camera_name,
        crate::routes::camera_routes::set_credentials,
        crate::routes::camera_routes::set_snapshot_method,
        crate::routes::camera_routes::set_multi_channel_detection,
        crate::routes::camera_routes::fix_snapshot_uri,
        crate::routes::media_routes::get_snapshot,
        crate::routes::media_routes::detect_objects,
        crate::routes::media_routes::get_recording_summary,
        crate::routes::media_routes::get_recording_list,
        crate::routes::media_routes::download_recording,
        crate::routes::config_routes::get_config,
        crate::routes::config_routes::reset_config,
        crate::routes::config_routes::get_available_detector_types,
        crate::routes::config_routes::update_detector_config,
        crate::routes::config_routes::update_manual_detection_config,
        crate::routes::config_routes::update_polling_config,
    ),
    components(
        schemas(
            LoginRequest,
            LoginResponse,
            CurrentUserResponse,
            AuthErrorResponse,
            AuthConfig,
            CameraResponse,
            AddCameraRequest,
            SetCameraNameRequest,
            SetCredentialsRequest,
            SetSnapshotMethodRequest,
            SetMultiChannelDetectionRequest,
            DetectedObjectResponse,
            DetectionResponse,
            Recording,
            RecordingSummary,
            UpdateDetectorConfigRequest,
            UpdateManualDetectionConfigRequest,
            UpdatePollingConfigRequest,
            GenericMessageResponse,
            BotConfig,
            DetectorConfig,
            ManualDetectionConfig,
            ObjectClass,
        )
    ),
    tags(
        (name = "Auth", description = "Authentication and session management"),
        (name = "Cameras", description = "Camera management endpoints"),
        (name = "Media", description = "Snapshots and YOLO detection endpoints"),
        (name = "Config", description = "Application and detector configuration")
    ),
    modifiers(&SecurityAddon),
    security(
        ("bearer_auth" = [])
    )
)]
pub struct ApiDoc;

pub mod auth_middleware;
pub mod dto;
pub mod routes;
pub mod state;

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let public_routes = Router::new().route("/api/v1/auth/login", axum::routing::post(login));

    let protected_routes = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        // Auth routes (protected)
        .route("/api/v1/auth/logout", axum::routing::post(logout))
        .route("/api/v1/auth/me", axum::routing::get(current_user))
        // Camera routes
        .route(
            "/api/v1/cameras",
            axum::routing::get(get_cameras).post(add_camera),
        )
        .route(
            "/api/v1/cameras/{id}",
            axum::routing::get(get_camera).delete(delete_camera),
        )
        .route(
            "/api/v1/cameras/{id}/name",
            axum::routing::put(set_camera_name),
        )
        .route(
            "/api/v1/cameras/{id}/credentials",
            axum::routing::put(set_credentials),
        )
        .route(
            "/api/v1/cameras/{id}/snapshot-method",
            axum::routing::put(set_snapshot_method),
        )
        .route(
            "/api/v1/cameras/{id}/multi-channel-detection",
            axum::routing::put(set_multi_channel_detection),
        )
        .route(
            "/api/v1/cameras/{id}/fix-snapshot-uri",
            axum::routing::post(fix_snapshot_uri),
        )
        // Media routes
        .route(
            "/api/v1/cameras/{id}/snapshot",
            axum::routing::get(get_snapshot),
        )
        .route(
            "/api/v1/cameras/{id}/detect",
            axum::routing::post(detect_objects),
        )
        .route(
            "/api/v1/cameras/{id}/recordings/summary",
            axum::routing::get(get_recording_summary),
        )
        .route(
            "/api/v1/cameras/{id}/recordings",
            axum::routing::get(get_recording_list),
        )
        .route(
            "/api/v1/cameras/{id}/recordings/download",
            axum::routing::get(download_recording),
        )
        // Config routes
        .route("/api/v1/config", axum::routing::get(get_config))
        .route("/api/v1/config/reset", axum::routing::post(reset_config))
        .route(
            "/api/v1/config/detector-types",
            axum::routing::get(get_available_detector_types),
        )
        .route(
            "/api/v1/config/detector",
            axum::routing::patch(update_detector_config),
        )
        .route(
            "/api/v1/config/manual-detection",
            axum::routing::patch(update_manual_detection_config),
        )
        .route(
            "/api/v1/config/polling",
            axum::routing::patch(update_polling_config),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .fallback(static_handler)
        .layer(cors)
        .with_state(state)
}

pub async fn run_server(state: AppState, addr: std::net::SocketAddr) -> anyhow::Result<()> {
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("HTTP API listening on http://{}", addr);
    tracing::info!("Swagger UI available at http://{}/swagger-ui", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
