use app_services::{AuthService, CameraService, ConfigService, MediaService};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub auth_service: Arc<AuthService>,
    pub camera_service: Arc<CameraService>,
    pub media_service: Arc<MediaService>,
    pub config_service: Arc<ConfigService>,
}
