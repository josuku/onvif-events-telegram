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

#[allow(clippy::too_many_arguments)]
pub async fn ffmpeg_trim_and_convert(
    input: &Path,
    output: &Path,
    input_format: Option<&str>,
    offset_secs: f64,
    duration_secs: f64,
    video_tag: Option<&str>,
    has_audio: bool,
) -> anyhow::Result<()> {
    let input_str = input.to_str().unwrap();
    let output_str = output.to_str().unwrap();
    let offset_str = offset_secs.to_string();
    let duration_str = duration_secs.to_string();

    let common_prefix = || -> Vec<String> {
        let mut args = Vec::new();
        if let Some(fmt) = input_format {
            args.push("-f".into());
            args.push(fmt.into());
        }
        if offset_secs > 0.05 {
            args.push("-ss".into());
            args.push(offset_str.clone());
        }
        args.push("-i".into());
        args.push(input_str.into());
        args.push("-t".into());
        args.push(duration_str.clone());
        args
    };

    let common_suffix = || -> Vec<String> {
        let mut args = Vec::new();
        if has_audio {
            args.push("-c:a".into());
            args.push("aac".into());
            args.push("-b:a".into());
            args.push("64k".into());
        }
        args.push("-movflags".into());
        args.push("+faststart".into());
        args.push("-y".into());
        args.push(output_str.into());
        args
    };

    let mut fast_args = common_prefix();
    fast_args.push("-c:v".into());
    fast_args.push("copy".into());
    if let Some(tag) = video_tag {
        fast_args.push("-tag:v".into());
        fast_args.push(tag.into());
    }
    fast_args.extend(common_suffix());

    let fast_args_ref: Vec<&str> = fast_args.iter().map(String::as_str).collect();
    let fast_result = run_ffmpeg(&fast_args_ref).await;

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

    let mut fallback_args = common_prefix();
    fallback_args.push("-c:v".into());
    fallback_args.push("libx264".into());
    fallback_args.push("-preset".into());
    fallback_args.push("veryfast".into());
    fallback_args.push("-crf".into());
    fallback_args.push("23".into());
    fallback_args.extend(common_suffix());

    let fallback_args_ref: Vec<&str> = fallback_args.iter().map(String::as_str).collect();
    run_ffmpeg(&fallback_args_ref).await
}
