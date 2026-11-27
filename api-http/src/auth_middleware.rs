use crate::{dto::AuthErrorResponse, state::AppState};
use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    if !state.auth_service.is_auth_enabled() {
        return next.run(req).await;
    }

    let token = extract_token(&req);

    match token {
        Some(token_str) => match state.auth_service.verify_token(&token_str) {
            Ok(claims) => {
                req.extensions_mut().insert(claims);
                next.run(req).await
            }
            Err(e) => (
                StatusCode::UNAUTHORIZED,
                Json(AuthErrorResponse {
                    error: "Unauthorized".to_string(),
                    message: format!("Invalid or expired token: {e}"),
                }),
            )
                .into_response(),
        },
        None => (
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                error: "Unauthorized".to_string(),
                message: "Authentication required. Provide a valid Bearer token, cookie or token query parameter.".to_string(),
            }),
        )
            .into_response(),
    }
}

fn extract_token<B>(req: &axum::http::Request<B>) -> Option<String> {
    // 1. Check Authorization: Bearer <token>
    if let Some(auth_val) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_val.to_str() {
            if let Some(token) = auth_str
                .strip_prefix("Bearer ")
                .or_else(|| auth_str.strip_prefix("bearer "))
            {
                let trimmed = token.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    // 2. Check Cookie: token=<token>
    if let Some(cookie_val) = req.headers().get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_val.to_str() {
            for part in cookie_str.split(';') {
                let part = part.trim();
                if let Some(token) = part
                    .strip_prefix("token=")
                    .or_else(|| part.strip_prefix("auth_token="))
                {
                    let trimmed = token.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }

    // 3. Check Query parameter ?token=<token>
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(token) = pair.strip_prefix("token=") {
                let trimmed = token.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[test]
    fn test_extract_token_from_auth_header() {
        let req = Request::builder()
            .header(header::AUTHORIZATION, "Bearer my-jwt-token")
            .body(())
            .unwrap();
        assert_eq!(extract_token(&req), Some("my-jwt-token".to_string()));

        let req_lower = Request::builder()
            .header(header::AUTHORIZATION, "bearer my-jwt-token-2")
            .body(())
            .unwrap();
        assert_eq!(
            extract_token(&req_lower),
            Some("my-jwt-token-2".to_string())
        );
    }

    #[test]
    fn test_extract_token_from_cookie() {
        let req = Request::builder()
            .header(
                header::COOKIE,
                "other=123; token=cookie-token-abc; session=xyz",
            )
            .body(())
            .unwrap();
        assert_eq!(extract_token(&req), Some("cookie-token-abc".to_string()));

        let req_auth_cookie = Request::builder()
            .header(header::COOKIE, "auth_token=my-auth-cookie")
            .body(())
            .unwrap();
        assert_eq!(
            extract_token(&req_auth_cookie),
            Some("my-auth-cookie".to_string())
        );
    }

    #[test]
    fn test_extract_token_from_query() {
        let req = Request::builder()
            .uri("/swagger-ui?foo=bar&token=query-token-123&baz=456")
            .body(())
            .unwrap();
        assert_eq!(extract_token(&req), Some("query-token-123".to_string()));
    }

    #[test]
    fn test_extract_token_none_when_missing() {
        let req = Request::builder().uri("/api/v1/cameras").body(()).unwrap();
        assert_eq!(extract_token(&req), None);
    }
}
