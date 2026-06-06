pub mod domain;
pub mod helpers;
pub mod traits;

pub type CameraId = i64;
pub type SubscriptionId = i64;
pub type ChatId = i64;

pub fn make_caption(title: &str, name: &str, time: &chrono::DateTime<chrono::Utc>) -> String {
    let converted: chrono::DateTime<chrono::Local> = chrono::DateTime::from(*time);
    // println!("utc:{} local:{}", time, converted);
    format!(
        r#"
{}
Camera:{}
Time: {}"#,
        title.to_uppercase(),
        name,
        converted
    )
}
