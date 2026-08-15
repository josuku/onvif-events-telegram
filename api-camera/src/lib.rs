use anyhow::Context;
use chrono::{DateTime, Duration, Utc};

pub mod dahua_rpc_api_client;
pub mod dvrip_xmeye_client;

const MAX_FILES: usize = 5;
const MAX_DOWNLOAD_SECONDS: i64 = 300; // 5 minutes
const _MAX_FILE_SIZE_MB: i64 = 50; // Telegram bot limits

pub fn get_clip_interval(
    time: DateTime<Utc>,
    clip_time: Duration,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let start_time = time - clip_time;
    let mut end_time = time + clip_time;
    if end_time > Utc::now() {
        end_time = Utc::now();
    }
    (start_time, end_time)
}

pub async fn run_ffmpeg(args: &[&str]) -> anyhow::Result<()> {
    let result = tokio::process::Command::new("ffmpeg")
        .args(args)
        .output()
        .await
        .context("cannot execute ffmpeg")?;

    if !result.status.success() {
        anyhow::bail!(
            "error with ffmpeg: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
}
