use async_trait::async_trait;

use crate::domain::object::Object;

#[async_trait]
pub trait ObjectDetector: Send + Sync {
    fn detect(&mut self, image: &[u8]) -> anyhow::Result<Vec<Object>>;
}
