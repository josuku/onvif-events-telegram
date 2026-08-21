use crate::domain::{camera::CameraEvent, error_message::ErrorMessage};
use tokio::sync::broadcast;

#[derive(Clone)]
pub enum EventBusMessage {
    CameraEvent(CameraEvent),
    Error(ErrorMessage),
}

pub struct EventBus {
    pub tx: broadcast::Sender<EventBusMessage>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, event: EventBusMessage) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventBusMessage> {
        self.tx.subscribe()
    }
}
