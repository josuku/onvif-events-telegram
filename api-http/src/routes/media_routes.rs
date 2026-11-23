use crate::{
    dto::{DetectedObjectResponse, DetectionResponse, GenericMessageResponse},
    state::AppState,
};
use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use tracing::error;

#[utoipa::path(
    get,
    path = "/api/v1/cameras/{id}/snapshot",
    tag = "Media",
    params(
        ("id" = i64, Path, description = "Camera ID")
    ),
    responses(
        (status = 200, description = "Camera snapshot image", content_type = "image/jpeg"),
        (status = 404, description = "Camera not found or failed", body = GenericMessageResponse)
    )
)]
pub async fn get_snapshot(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, (StatusCode, Json<GenericMessageResponse>)> {
    match state.media_service.get_snapshot(id).await {
        Ok(bytes) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
            Ok((headers, Body::from(bytes)).into_response())
        }
        Err(err) => {
            error!("Error getting snapshot for camera {}: {}", id, err);
            Err((
                StatusCode::NOT_FOUND,
                Json(GenericMessageResponse {
                    success: false,
                    message: err.to_string(),
                }),
            ))
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/cameras/{id}/detect",
    tag = "Media",
    params(
        ("id" = i64, Path, description = "Camera ID")
    ),
    responses(
        (status = 200, description = "Detected objects with YOLO", body = DetectionResponse),
        (status = 400, description = "Detection error", body = GenericMessageResponse)
    )
)]
pub async fn detect_objects(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<DetectionResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    match state.media_service.detect_objects(id).await {
        Ok(outcome) => {
            let detected = outcome
                .objects
                .into_iter()
                .map(|obj| DetectedObjectResponse {
                    class: obj.class.to_string(),
                    confidence: obj.confidence,
                    x1: obj.bbox.x1,
                    y1: obj.bbox.y1,
                    x2: obj.bbox.x2,
                    y2: obj.bbox.y2,
                })
                .collect();

            Ok(Json(DetectionResponse {
                camera_id: outcome.camera_id,
                camera_name: outcome.camera_name,
                objects: detected,
            }))
        }
        Err(err) => {
            error!("Error running detection for camera {}: {}", id, err);
            Err((
                StatusCode::BAD_REQUEST,
                Json(GenericMessageResponse {
                    success: false,
                    message: err.to_string(),
                }),
            ))
        }
    }
}
