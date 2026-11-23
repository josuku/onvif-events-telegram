use crate::{
    dto::{
        AddCameraRequest, CameraResponse, GenericMessageResponse, SetCameraNameRequest,
        SetCredentialsRequest, SetMultiChannelDetectionRequest, SetSnapshotMethodRequest,
    },
    state::AppState,
};
use app_core::domain::camera::SnapshotMethod;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use tracing::error;

#[utoipa::path(
    get,
    path = "/api/v1/cameras",
    tag = "Cameras",
    responses(
        (status = 200, description = "List of all registered cameras", body = Vec<CameraResponse>)
    )
)]
pub async fn get_cameras(State(state): State<AppState>) -> Json<Vec<CameraResponse>> {
    let dtos = state.camera_service.get_cameras_dto().await;
    Json(dtos.into_iter().map(CameraResponse::from).collect())
}

#[utoipa::path(
    get,
    path = "/api/v1/cameras/{id}",
    tag = "Cameras",
    params(
        ("id" = i64, Path, description = "Camera ID")
    ),
    responses(
        (status = 200, description = "Camera details", body = CameraResponse),
        (status = 404, description = "Camera not found", body = GenericMessageResponse)
    )
)]
pub async fn get_camera(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<CameraResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    let dtos = state.camera_service.get_cameras_dto().await;
    if let Some(dto) = dtos.into_iter().find(|c| c.id == id) {
        Ok(Json(CameraResponse::from(dto)))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(GenericMessageResponse {
                success: false,
                message: format!("Camera with id {} not found", id),
            }),
        ))
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/cameras",
    tag = "Cameras",
    request_body = AddCameraRequest,
    responses(
        (status = 201, description = "Camera added successfully", body = GenericMessageResponse),
        (status = 400, description = "Error adding camera", body = GenericMessageResponse)
    )
)]
pub async fn add_camera(
    State(state): State<AppState>,
    Json(req): Json<AddCameraRequest>,
) -> Result<(StatusCode, Json<GenericMessageResponse>), (StatusCode, Json<GenericMessageResponse>)>
{
    match state
        .camera_service
        .add_camera(&req.uri, &req.username, &req.password)
        .await
    {
        Ok(camera_id) => Ok((
            StatusCode::CREATED,
            Json(GenericMessageResponse {
                success: true,
                message: format!("Camera added successfully with id: {}", camera_id),
            }),
        )),
        Err(err) => {
            error!("Error adding camera: {}", err);
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
    delete,
    path = "/api/v1/cameras/{id}",
    tag = "Cameras",
    params(
        ("id" = i64, Path, description = "Camera ID")
    ),
    responses(
        (status = 200, description = "Camera deleted successfully", body = GenericMessageResponse),
        (status = 400, description = "Error deleting camera", body = GenericMessageResponse)
    )
)]
pub async fn delete_camera(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<GenericMessageResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    match state.camera_service.delete_camera(id).await {
        Ok(_) => Ok(Json(GenericMessageResponse {
            success: true,
            message: format!("Camera {} deleted successfully", id),
        })),
        Err(err) => {
            error!("Error deleting camera {}: {}", id, err);
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
    put,
    path = "/api/v1/cameras/{id}/name",
    tag = "Cameras",
    params(
        ("id" = i64, Path, description = "Camera ID")
    ),
    request_body = SetCameraNameRequest,
    responses(
        (status = 200, description = "Camera name updated", body = GenericMessageResponse),
        (status = 400, description = "Error updating name", body = GenericMessageResponse)
    )
)]
pub async fn set_camera_name(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<SetCameraNameRequest>,
) -> Result<Json<GenericMessageResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    match state.camera_service.set_camera_name(id, &req.name).await {
        Ok(_) => Ok(Json(GenericMessageResponse {
            success: true,
            message: "Camera name updated successfully".to_string(),
        })),
        Err(err) => Err((
            StatusCode::BAD_REQUEST,
            Json(GenericMessageResponse {
                success: false,
                message: err.to_string(),
            }),
        )),
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/cameras/{id}/credentials",
    tag = "Cameras",
    params(
        ("id" = i64, Path, description = "Camera ID")
    ),
    request_body = SetCredentialsRequest,
    responses(
        (status = 200, description = "Camera credentials updated", body = GenericMessageResponse),
        (status = 400, description = "Error updating credentials", body = GenericMessageResponse)
    )
)]
pub async fn set_credentials(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<SetCredentialsRequest>,
) -> Result<Json<GenericMessageResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    match state
        .camera_service
        .set_credentials(id, &req.username, &req.password)
        .await
    {
        Ok(_) => Ok(Json(GenericMessageResponse {
            success: true,
            message: "Credentials updated successfully".to_string(),
        })),
        Err(err) => Err((
            StatusCode::BAD_REQUEST,
            Json(GenericMessageResponse {
                success: false,
                message: err.to_string(),
            }),
        )),
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/cameras/{id}/snapshot-method",
    tag = "Cameras",
    params(
        ("id" = i64, Path, description = "Camera ID")
    ),
    request_body = SetSnapshotMethodRequest,
    responses(
        (status = 200, description = "Snapshot method updated", body = GenericMessageResponse),
        (status = 400, description = "Invalid method or error", body = GenericMessageResponse)
    )
)]
pub async fn set_snapshot_method(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<SetSnapshotMethodRequest>,
) -> Result<Json<GenericMessageResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    let method: SnapshotMethod = req.method.parse().map_err(|err: String| {
        (
            StatusCode::BAD_REQUEST,
            Json(GenericMessageResponse {
                success: false,
                message: err,
            }),
        )
    })?;

    match state.camera_service.set_snapshot_method(id, method).await {
        Ok(_) => Ok(Json(GenericMessageResponse {
            success: true,
            message: format!("Snapshot method updated to '{}'", method),
        })),
        Err(err) => Err((
            StatusCode::BAD_REQUEST,
            Json(GenericMessageResponse {
                success: false,
                message: err.to_string(),
            }),
        )),
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/cameras/{id}/multi-channel-detection",
    tag = "Cameras",
    params(
        ("id" = i64, Path, description = "Camera ID")
    ),
    request_body = SetMultiChannelDetectionRequest,
    responses(
        (status = 200, description = "Multi-channel detection updated", body = GenericMessageResponse),
        (status = 400, description = "Error updating setting", body = GenericMessageResponse)
    )
)]
pub async fn set_multi_channel_detection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<SetMultiChannelDetectionRequest>,
) -> Result<Json<GenericMessageResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    match state
        .camera_service
        .set_multi_channel_detection(id, req.enable)
        .await
    {
        Ok(_) => Ok(Json(GenericMessageResponse {
            success: true,
            message: format!("Multi-channel detection set to {}", req.enable),
        })),
        Err(err) => Err((
            StatusCode::BAD_REQUEST,
            Json(GenericMessageResponse {
                success: false,
                message: err.to_string(),
            }),
        )),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/cameras/{id}/fix-snapshot-uri",
    tag = "Cameras",
    params(
        ("id" = i64, Path, description = "Camera ID")
    ),
    responses(
        (status = 200, description = "Snapshot URI fixed", body = GenericMessageResponse),
        (status = 400, description = "Error fixing URI", body = GenericMessageResponse)
    )
)]
pub async fn fix_snapshot_uri(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<GenericMessageResponse>, (StatusCode, Json<GenericMessageResponse>)> {
    match state.camera_service.fix_snapshot_uri(id).await {
        Ok(uri) => Ok(Json(GenericMessageResponse {
            success: true,
            message: format!("Snapshot URI resolved/fixed: {}", uri),
        })),
        Err(err) => Err((
            StatusCode::BAD_REQUEST,
            Json(GenericMessageResponse {
                success: false,
                message: err.to_string(),
            }),
        )),
    }
}
