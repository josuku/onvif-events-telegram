use crate::dtos::{AuthClaims, LoginRequest, LoginResponse};
use app_core::domain::config::AuthConfig;
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Invalid username or password")]
    InvalidCredentials,
    #[error("Token creation failed: {0}")]
    TokenCreation(String),
    #[error("Token expired")]
    TokenExpired,
    #[error("Invalid token: {0}")]
    InvalidToken(String),
}

pub struct AuthService {
    config: AuthConfig,
}

impl AuthService {
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }

    pub fn is_auth_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn token_expiration_seconds(&self) -> u64 {
        self.config.token_expiration_hours * 3600
    }

    pub fn login(&self, req: &LoginRequest) -> Result<LoginResponse, AuthError> {
        if req.username != self.config.username || req.password != self.config.password {
            return Err(AuthError::InvalidCredentials);
        }

        let now = Utc::now().timestamp() as usize;
        let expires_in = self.token_expiration_seconds();
        let exp = now + expires_in as usize;

        let claims = AuthClaims {
            sub: req.username.clone(),
            exp,
            iat: now,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.config.jwt_secret.as_bytes()),
        )
        .map_err(|e| AuthError::TokenCreation(e.to_string()))?;

        Ok(LoginResponse {
            token,
            token_type: "Bearer".to_string(),
            expires_in,
        })
    }

    pub fn verify_token(&self, token: &str) -> Result<AuthClaims, AuthError> {
        let mut validation = Validation::default();
        validation.validate_exp = true;

        let token_data = decode::<AuthClaims>(
            token,
            &DecodingKey::from_secret(self.config.jwt_secret.as_bytes()),
            &validation,
        )
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
            _ => AuthError::InvalidToken(e.to_string()),
        })?;

        Ok(token_data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_auth_service(enabled: bool) -> AuthService {
        AuthService::new(AuthConfig {
            enabled,
            username: "admin".to_string(),
            password: "supersecretpassword".to_string(),
            jwt_secret: "test-jwt-secret-key-1234567890".to_string(),
            token_expiration_hours: 1,
        })
    }

    #[test]
    fn test_login_success() {
        let auth = create_test_auth_service(true);
        let req = LoginRequest {
            username: "admin".to_string(),
            password: "supersecretpassword".to_string(),
        };

        let res = auth.login(&req).expect("Login should succeed");
        assert_eq!(res.token_type, "Bearer");
        assert_eq!(res.expires_in, 3600);
        assert!(!res.token.is_empty());

        let claims = auth.verify_token(&res.token).expect("Token should verify");
        assert_eq!(claims.sub, "admin");
    }

    #[test]
    fn test_login_wrong_credentials() {
        let auth = create_test_auth_service(true);

        let wrong_user = LoginRequest {
            username: "wrong".to_string(),
            password: "supersecretpassword".to_string(),
        };
        assert!(matches!(
            auth.login(&wrong_user),
            Err(AuthError::InvalidCredentials)
        ));

        let wrong_pass = LoginRequest {
            username: "admin".to_string(),
            password: "wrong".to_string(),
        };
        assert!(matches!(
            auth.login(&wrong_pass),
            Err(AuthError::InvalidCredentials)
        ));
    }

    #[test]
    fn test_verify_invalid_token() {
        let auth = create_test_auth_service(true);
        assert!(matches!(
            auth.verify_token("invalid.jwt.token"),
            Err(AuthError::InvalidToken(_))
        ));
    }

    #[test]
    fn test_verify_token_with_different_secret() {
        let auth1 = create_test_auth_service(true);
        let auth2 = AuthService::new(AuthConfig {
            enabled: true,
            username: "admin".to_string(),
            password: "supersecretpassword".to_string(),
            jwt_secret: "a-different-secret-key-987654321".to_string(),
            token_expiration_hours: 1,
        });

        let req = LoginRequest {
            username: "admin".to_string(),
            password: "supersecretpassword".to_string(),
        };
        let res = auth1.login(&req).unwrap();

        assert!(matches!(
            auth2.verify_token(&res.token),
            Err(AuthError::InvalidToken(_))
        ));
    }
}
