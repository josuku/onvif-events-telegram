use crate::domain::{camera::CameraEventType, object::Object};
use tracing::debug;

pub mod domain;
pub mod helpers;
pub mod traits;

pub type CameraId = i64;
pub type SubscriptionId = i64;
pub type ChatId = i64;
pub type MessageId = i32;

pub fn make_caption(
    title: &str,
    name: &str,
    time: &chrono::DateTime<chrono::Utc>,
    event_type: Option<CameraEventType>,
    objects: &[Object],
) -> String {
    let converted: chrono::DateTime<chrono::Local> = chrono::DateTime::from(*time);
    debug!("utc:{} local:{}", time, converted);

    if let Some(event_type) = event_type {
        caption_with_type(title, name, &converted, event_type, objects)
    } else {
        caption_without_type(title, name, &converted, objects)
    }
}

fn caption_without_type(
    title: &str,
    name: &str,
    time: &chrono::DateTime<chrono::Local>,
    objects: &[Object],
) -> String {
    format!(
        r#"
{}
Camera: {}
Time: {}{}"#,
        title.to_uppercase(),
        name,
        format_time(time),
        format_objects(objects),
    )
}

fn caption_with_type(
    title: &str,
    name: &str,
    time: &chrono::DateTime<chrono::Local>,
    event_type: CameraEventType,
    objects: &[Object],
) -> String {
    format!(
        r#"
{}
Camera: {}
Type: {}
Time: {}{}"#,
        title.to_uppercase(),
        name,
        event_type,
        format_time(time),
        format_objects(objects),
    )
}

fn format_time(time: &chrono::DateTime<chrono::Local>) -> String {
    time.format("%Y-%m-%d %H:%M:%S %:z").to_string()
}

fn format_objects(objects: &[Object]) -> String {
    if objects.is_empty() {
        String::new()
    } else {
        let objects = objects
            .iter()
            .map(|o| format!("• {}", o))
            .collect::<Vec<_>>()
            .join("\n");

        format!("\nObjects:\n{}", objects)
    }
}
