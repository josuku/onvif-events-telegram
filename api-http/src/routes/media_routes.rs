use crate::{
    dto::{
        DetectedObjectResponse, DetectionResponse, GenericMessageResponse, RecordingListQuery,
        RecordingSummaryQuery,
    },
    state::AppState,
};
use app_core::domain::camera::{Recording, RecordingSummary};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use tracing::error;

fn parse_query_date(raw: Option<&str>) -> DateTime<Utc> {
    if let Some(s) = raw {
        let s = s.trim();
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return dt.with_timezone(&Utc);
        }
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            return dt.and_utc();
        }
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return dt.and_utc();
        }
        if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            return d.and_hms_opt(0, 0, 0).unwrap().and_utc();
        }
    }
    Utc::now()
}

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

#[utoipa::path(
    get,
    path = "/api/v1/cameras/{id}/recordings/summary",
    tag = "Media",
    params(
        ("id" = i64, Path, description = "Camera ID"),
        ("days" = Option<i64>, Query, description = "Number of days in the past to query (default 7)")
    ),
    responses(
        (status = 200, description = "Recording summary", body = RecordingSummary),
        (status = 404, description = "Camera not found or API error", body = GenericMessageResponse)
    )
)]
pub async fn get_recording_summary(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<RecordingSummaryQuery>,
) -> Result<Json<RecordingSummary>, (StatusCode, Json<GenericMessageResponse>)> {
    let days = query.days.unwrap_or(7);
    match state
        .media_service
        .get_recording_summary(id, Some(days))
        .await
    {
        Ok(summary) => Ok(Json(summary)),
        Err(err) => {
            error!("Error getting recording summary for camera {}: {}", id, err);
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
    get,
    path = "/api/v1/cameras/{id}/recordings",
    tag = "Media",
    params(
        ("id" = i64, Path, description = "Camera ID"),
        ("date" = Option<String>, Query, description = "Date string (e.g. 2026-11-23, ISO8601)"),
        ("clip_seconds" = Option<u64>, Query, description = "Clip search window in seconds")
    ),
    responses(
        (status = 200, description = "List of recording files", body = Vec<Recording>),
        (status = 404, description = "Camera not found or API error", body = GenericMessageResponse)
    )
)]
pub async fn get_recording_list(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<RecordingListQuery>,
) -> Result<Json<Vec<Recording>>, (StatusCode, Json<GenericMessageResponse>)> {
    let date = parse_query_date(query.date.as_deref());
    match state
        .media_service
        .get_recording_list(id, date, query.clip_seconds)
        .await
    {
        Ok(recordings) => Ok(Json(recordings)),
        Err(err) => {
            error!("Error getting recording list for camera {}: {}", id, err);
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
    get,
    path = "/api/v1/cameras/{id}/recordings/download",
    tag = "Media",
    params(
        ("id" = i64, Path, description = "Camera ID"),
        ("date" = Option<String>, Query, description = "Date string (e.g. 2026-11-23, ISO8601)"),
        ("clip_seconds" = Option<u64>, Query, description = "Clip duration in seconds")
    ),
    responses(
        (status = 200, description = "Recording video file", content_type = "video/mp4"),
        (status = 404, description = "Recording not found or download error", body = GenericMessageResponse)
    )
)]
pub async fn download_recording(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<RecordingListQuery>,
) -> Result<Response, (StatusCode, Json<GenericMessageResponse>)> {
    let date = parse_query_date(query.date.as_deref());
    match state
        .media_service
        .download_recording(id, date, query.clip_seconds)
        .await
    {
        Ok(file_path) => match tokio::fs::read(&file_path).await {
            Ok(bytes) => {
                let filename = std::path::Path::new(&file_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("recording.mp4");

                let mut headers = HeaderMap::new();
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
                if let Ok(val) =
                    HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename))
                {
                    headers.insert(header::CONTENT_DISPOSITION, val);
                }
                tokio::fs::remove_file(&file_path).await.ok();
                Ok((headers, Body::from(bytes)).into_response())
            }
            Err(err) => {
                error!("Error reading downloaded recording {}: {}", file_path, err);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(GenericMessageResponse {
                        success: false,
                        message: format!("Failed to read downloaded file: {}", err),
                    }),
                ))
            }
        },
        Err(err) => {
            error!("Error downloading recording for camera {}: {}", id, err);
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
