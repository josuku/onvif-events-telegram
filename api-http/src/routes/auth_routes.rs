use crate::{
    dto::{
        AuthErrorResponse, CurrentUserResponse, GenericMessageResponse, LoginRequest, LoginResponse,
    },
    state::AppState,
};
use app_services::{AuthClaims, LoginRequest as ServiceLoginRequest};
use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = LoginResponse),
        (status = 401, description = "Invalid credentials", body = AuthErrorResponse)
    ),
    tag = "Auth",
    security()
)]
pub async fn login(State(state): State<AppState>, Json(payload): Json<LoginRequest>) -> Response {
    let service_req = ServiceLoginRequest {
        username: payload.username,
        password: payload.password,
    };

    match state.auth_service.login(&service_req) {
        Ok(res) => {
            let cookie_val = format!(
                "token={}; HttpOnly; Path=/; SameSite=Lax; Max-Age={}",
                res.token, res.expires_in
            );

            let mut headers = HeaderMap::new();
            if let Ok(val) = HeaderValue::from_str(&cookie_val) {
                headers.insert(header::SET_COOKIE, val);
            }

            (
                StatusCode::OK,
                headers,
                Json(LoginResponse {
                    token: res.token,
                    token_type: res.token_type,
                    expires_in: res.expires_in,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                error: "Unauthorized".to_string(),
                message: e.to_string(),
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    responses(
        (status = 200, description = "Logout successful", body = GenericMessageResponse)
    ),
    tag = "Auth",
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn logout() -> Response {
    let cookie_val = "token=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0";
    let mut headers = HeaderMap::new();
    if let Ok(val) = HeaderValue::from_str(cookie_val) {
        headers.insert(header::SET_COOKIE, val);
    }

    (
        StatusCode::OK,
        headers,
        Json(GenericMessageResponse {
            success: true,
            message: "Logged out successfully".to_string(),
        }),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    responses(
        (status = 200, description = "Current authenticated user", body = CurrentUserResponse),
        (status = 401, description = "Unauthorized", body = AuthErrorResponse)
    ),
    tag = "Auth",
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn current_user(Extension(claims): Extension<AuthClaims>) -> Response {
    Json(CurrentUserResponse {
        username: claims.sub,
        expires_at: claims.exp,
    })
    .into_response()
}
