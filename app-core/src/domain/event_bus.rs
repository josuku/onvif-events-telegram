use crate::domain::camera::CameraEvent;
use tokio::sync::broadcast;

pub struct EventBus {
    pub tx: broadcast::Sender<CameraEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, event: CameraEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CameraEvent> {
        self.tx.subscribe()
    }
}
