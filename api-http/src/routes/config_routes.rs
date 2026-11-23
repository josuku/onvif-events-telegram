use crate::{
    dto::{GenericMessageResponse, UpdateDetectorConfigRequest, UpdatePollingConfigRequest},
    state::AppState,
};
use app_core::domain::{config::BotConfig, object::ObjectClass};
use axum::{Json, extract::State, http::StatusCode};

#[utoipa::path(
    get,
    path = "/api/v1/config",
    tag = "Config",
    responses(
        (status = 200, description = "Current configuration", body = BotConfig)
    )
)]
pub async fn get_config(State(state): State<AppState>) -> Json<BotConfig> {
    Json(state.config_service.get_config().await)
}

#[utoipa::path(
    post,
    path = "/api/v1/config/reset",
    tag = "Config",
    responses(
        (status = 200, description = "Config reset to default", body = BotConfig)
    )
)]
pub async fn reset_config(State(state): State<AppState>) -> Json<BotConfig> {
    Json(state.config_service.reset_config().await)
}

#[utoipa::path(
    get,
    path = "/api/v1/config/detector-types",
    tag = "Config",
    responses(
        (status = 200, description = "Available object classes for detection", body = Vec<ObjectClass>)
    )
)]
pub async fn get_available_detector_types(State(state): State<AppState>) -> Json<Vec<ObjectClass>> {
    Json(state.config_service.get_available_detector_types().await)
}

#[utoipa::path(
    patch,
    path = "/api/v1/config/detector",
    tag = "Config",
    request_body = UpdateDetectorConfigRequest,
    responses(
        (status = 200, description = "Detector config updated", body = GenericMessageResponse),
        (status = 400, description = "Update failed", body = GenericMessageResponse)
    )
)]
pub async fn update_detector_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateDetectorConfigRequest>,
) -> Result<Json<GenericMessageResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    if let Some(enable) = req.enable {
        state
            .config_service
            .set_detector_enable(enable)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(GenericMessageResponse {
                        success: false,
                        message: e.to_string(),
                    }),
                )
            })?;
    }
    if let Some(min_conf) = req.min_confidence {
        state
            .config_service
            .set_detector_min_confidence(min_conf)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(GenericMessageResponse {
                        success: false,
                        message: e.to_string(),
                    }),
                )
            })?;
    }
    if let Some(types) = req.types {
        state
            .config_service
            .set_detector_types(types)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(GenericMessageResponse {
                        success: false,
                        message: e.to_string(),
                    }),
                )
            })?;
    }

    Ok(Json(GenericMessageResponse {
        success: true,
        message: "Detector configuration updated successfully".to_string(),
    }))
}

#[utoipa::path(
    patch,
    path = "/api/v1/config/polling",
    tag = "Config",
    request_body = UpdatePollingConfigRequest,
    responses(
        (status = 200, description = "Polling config updated", body = GenericMessageResponse),
        (status = 400, description = "Update failed", body = GenericMessageResponse)
    )
)]
pub async fn update_polling_config(
    State(state): State<AppState>,
    Json(req): Json<UpdatePollingConfigRequest>,
) -> Result<Json<GenericMessageResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    if let Some(secs) = req.polling_seconds {
        state
            .config_service
            .set_polling_seconds(secs)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(GenericMessageResponse {
                        success: false,
                        message: e.to_string(),
                    }),
                )
            })?;
    }
    if let Some(secs) = req.between_seconds {
        state
            .config_service
            .set_between_seconds(secs)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(GenericMessageResponse {
                        success: false,
                        message: e.to_string(),
                    }),
                )
            })?;
    }
    if let Some(send_errs) = req.send_errors {
        state
            .config_service
            .set_send_errors(send_errs)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(GenericMessageResponse {
                        success: false,
                        message: e.to_string(),
                    }),
                )
            })?;
    }
    if let Some(auto_renew) = req.auto_renewal {
        state
            .config_service
            .set_auto_renewal(auto_renew)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(GenericMessageResponse {
                        success: false,
                        message: e.to_string(),
                    }),
                )
            })?;
    }
    if let Some(clip_secs) = req.recording_clip {
        state
            .config_service
            .set_recording_clip(clip_secs)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(GenericMessageResponse {
                        success: false,
                        message: e.to_string(),
                    }),
                )
            })?;
    }

    Ok(Json(GenericMessageResponse {
        success: true,
        message: "Polling configuration updated successfully".to_string(),
    }))
}
