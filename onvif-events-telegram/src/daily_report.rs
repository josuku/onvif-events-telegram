use app_core::traits::notifier::Notifier;
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use tracing::info;

pub async fn manage_daily_report(
    notifier: Arc<dyn Notifier>,
    repository: Arc<MemoryRepository>,
    last_polling: chrono::DateTime<chrono::Local>,
) {
    let now = chrono::Local::now();
    let chat_ids = repository.get_daily_report_subscriptors().await;
    if !chat_ids.is_empty() && last_polling.date_naive() != now.date_naive() {
        info!("Sending daily report: {}", now.format("%Y-%m-%d %H:%M:%S"));

        let mut report = String::new();
        report.push_str(&format!("Daily report {}\n", last_polling.date_naive()));

        for camera in repository.get_sorted_cameras().await {
            let notifications = repository.get_today_camera_notifications(camera.id).await;
            let mut status = "";
            if !camera.client.connected() {
                status = "\n (disconnected)";
            }
            let mut last_sync = "".to_string();
            if let Some(last_polling_time) =
                repository.get_last_polling_from_camera(camera.id).await
            {
                last_sync = format!("\n   (sync: {})", time_ago(now.to_utc(), last_polling_time));
            }

            report.push_str(&format!(
                " - Camera {}-{}: {} detections{}{}\n",
                camera.id,
                camera.name,
                notifications.len(),
                status,
                last_sync,
            ));
        }

        notifier.send_text_message(report, chat_ids).await;

        repository.clear_today_notifications().await;
    }
}

fn time_ago(to: chrono::DateTime<chrono::Utc>, from: chrono::DateTime<chrono::Utc>) -> String {
    let duration = to.signed_duration_since(from);

    if duration.num_seconds() < 60 {
        format!("{} secs ago", duration.num_seconds())
    } else if duration.num_minutes() < 60 {
        format!("{} mins ago", duration.num_minutes())
    } else if duration.num_hours() < 24 {
        format!("{} hours ago", duration.num_hours())
    } else {
        format!("{} days ago", duration.num_days())
    }
}
