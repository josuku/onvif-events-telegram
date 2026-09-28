use std::path::Path;

use anyhow::Context;
use chrono::{DateTime, Duration, Utc};

pub mod dahua_rpc_api_client;
pub mod dvrip_xmeye_client;
pub mod hikvision_isapi_client;

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

pub async fn ffmpeg_trim_and_convert(
    input: &Path,
    output: &Path,
    offset_secs: f64,
    duration_secs: f64,
) -> anyhow::Result<()> {
    let mut args: Vec<String> = Vec::new();
    if offset_secs > 0.05 {
        args.push("-ss".into());
        args.push(offset_secs.to_string());
    }
    args.push("-i".into());
    args.push(input.to_str().unwrap().into());
    args.push("-t".into());
    args.push(duration_secs.to_string());
    args.push("-c:v".into());
    args.push("copy".into());
    args.push("-c:a".into());
    args.push("aac".into());
    args.push("-b:a".into());
    args.push("64k".into());
    args.push("-movflags".into());
    args.push("+faststart".into());
    args.push("-y".into());
    args.push(output.to_str().unwrap().into());
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let fast_result = run_ffmpeg(&args_ref).await;

    let fast_ok = fast_result.is_ok()
        && tokio::fs::metadata(output)
            .await
            .map(|m| m.len() > 0)
            .unwrap_or(false);

    if fast_ok {
        return Ok(());
    }

    tracing::warn!(
        "error doing fast remux ({:?}), trying to decoding as fallback",
        fast_result.err()
    );

    args.clear();
    if offset_secs > 0.05 {
        args.push("-ss".into());
        args.push(offset_secs.to_string());
    }
    args.push("-i".into());
    args.push(input.to_str().unwrap().into());
    args.push("-t".into());
    args.push(duration_secs.to_string());
    args.push("-c:v".into());
    args.push("libx264".into());
    args.push("-preset".into());
    args.push("veryfast".into());
    args.push("-crf".into());
    args.push("23".into());
    args.push("-c:a".into());
    args.push("aac".into());
    args.push("-b:a".into());
    args.push("64k".into());
    args.push("-movflags".into());
    args.push("+faststart".into());
    args.push("-y".into());
    args.push(output.to_str().unwrap().into());
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();

    run_ffmpeg(&args_ref).await
}
