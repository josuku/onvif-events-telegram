use crate::domain::camera::CameraEventType;
use tracing::debug;

pub mod domain;
pub mod helpers;
pub mod traits;

pub type CameraId = i64;
pub type SubscriptionId = i64;
pub type ChatId = i64;

pub fn make_caption(
    title: &str,
    name: &str,
    time: &chrono::DateTime<chrono::Utc>,
    event_type: Option<CameraEventType>,
) -> String {
    let converted: chrono::DateTime<chrono::Local> = chrono::DateTime::from(*time);
    debug!("utc:{} local:{}", time, converted);

    if let Some(event_type) = event_type {
        caption_with_type(title, name, &converted, event_type)
    } else {
        caption_without_type(title, name, &converted)
    }
}

fn caption_without_type(title: &str, name: &str, time: &chrono::DateTime<chrono::Local>) -> String {
    format!(
        r#"
{}
Camera: {}
Time: {}"#,
        title.to_uppercase(),
        name,
        time
    )
}

fn caption_with_type(
    title: &str,
    name: &str,
    time: &chrono::DateTime<chrono::Local>,
    event_type: CameraEventType,
) -> String {
    format!(
        r#"
{}
Camera: {}
Type: {}
Time: {}"#,
        title.to_uppercase(),
        name,
        event_type,
        time
    )
}
