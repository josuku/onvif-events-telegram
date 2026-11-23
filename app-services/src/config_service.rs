use anyhow::Result;
use app_core::domain::{config::BotConfig, object::ObjectClass};
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use tracing::info;

pub struct ConfigService {
    repository: Arc<MemoryRepository>,
}

impl ConfigService {
    pub fn new(repository: Arc<MemoryRepository>) -> Self {
        Self { repository }
    }

    pub async fn get_config(&self) -> BotConfig {
        self.repository.get_config().await
    }

    pub async fn get_available_detector_types(&self) -> Vec<ObjectClass> {
        self.repository.get_detector_types().await
    }

    pub async fn reset_config(&self) -> BotConfig {
        info!("Resetting configuration to default");
        self.repository.reset_config().await;
        self.repository.get_config().await
    }

    pub async fn set_polling_seconds(&self, seconds: u64) -> Result<()> {
        info!("Updating polling seconds to {}", seconds);
        self.repository.config_polling_seconds(seconds, 0).await;
        Ok(())
    }

    pub async fn set_between_seconds(&self, seconds: u64) -> Result<()> {
        info!("Updating between seconds to {}", seconds);
        self.repository.config_between_seconds(seconds, 0).await;
        Ok(())
    }

    pub async fn set_send_errors(&self, send_errors: bool) -> Result<()> {
        info!("Updating send_errors to {}", send_errors);
        self.repository.config_send_errors(send_errors, 0).await;
        Ok(())
    }

    pub async fn set_auto_renewal(&self, enable: bool) -> Result<()> {
        info!("Updating auto_renewal to {}", enable);
        self.repository.config_auto_renewal(enable, 0).await;
        Ok(())
    }

    pub async fn set_recording_clip(&self, seconds: u64) -> Result<()> {
        info!("Updating recording clip duration to {}s", seconds);
        self.repository.config_recording_clip(seconds, 0).await;
        Ok(())
    }

    pub async fn set_detector_enable(&self, enable: bool) -> Result<()> {
        info!("Updating detector enable to {}", enable);
        self.repository.config_detector_enable(enable, 0).await;
        Ok(())
    }

    pub async fn set_detector_min_confidence(&self, confidence: f32) -> Result<()> {
        info!("Updating detector min_confidence to {}", confidence);
        self.repository
            .config_detector_min_confidence(confidence, 0)
            .await;
        Ok(())
    }

    pub async fn set_detector_types(&self, types: Vec<ObjectClass>) -> Result<()> {
        info!("Updating detector types to {:?}", types);
        self.repository.config_detector_types(types, 0).await;
        Ok(())
    }

    pub async fn set_manual_detection(
        &self,
        enable: bool,
        interval: Option<u64>,
        types: Option<Vec<ObjectClass>>,
    ) -> Result<()> {
        info!(
            "Updating manual detection enable:{} interval:{:?} types:{:?}",
            enable, interval, types
        );
        self.repository
            .config_manual_detection(enable, interval, types, 0)
            .await;
        Ok(())
    }
}
