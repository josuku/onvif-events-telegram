use crate::domain::object::{Object, ObjectClass};
use async_trait::async_trait;

#[async_trait]
pub trait ObjectDetector: Send + Sync {
    fn detect(
        &mut self,
        image: &[u8],
        min_confidence: f32,
        types: &[ObjectClass],
    ) -> anyhow::Result<Vec<Object>>;
}
