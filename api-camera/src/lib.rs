use std::path::Path;

use anyhow::Context;
use chrono::{DateTime, Duration, Utc};

pub mod dahua_rpc_api_client;
pub mod dvrip_xmeye_client;
pub mod hikvision_isapi_client;

const MAX_FILES: usize = 5;
pub const MAX_DOWNLOAD_SECONDS: i64 = 600; // 10 minutes
pub const MAX_FILE_SIZE_BYTES: u64 = 49 * 1024 * 1024; // 49 MB to be safely within Telegram's 50 MB limit

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

pub fn get_clip_interval_clamped(
    time: DateTime<Utc>,
    clip_time: Duration,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let (start_time, mut end_time) = get_clip_interval(time, clip_time);
    let total_secs = (end_time - start_time).num_seconds();
    if total_secs > MAX_DOWNLOAD_SECONDS {
        end_time = start_time + Duration::seconds(MAX_DOWNLOAD_SECONDS);
        if end_time > Utc::now() {
            end_time = Utc::now();
        }
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

pub async fn get_video_duration(video_path: &Path) -> anyhow::Result<f64> {
    let output = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            video_path.to_str().unwrap_or_default(),
        ])
        .output()
        .await
        .context("cannot execute ffprobe")?;

    if !output.status.success() {
        anyhow::bail!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let out_str = String::from_utf8_lossy(&output.stdout);
    let dur = out_str
        .trim()
        .parse::<f64>()
        .context("failed to parse video duration from ffprobe")?;
    Ok(dur)
}

pub async fn split_video_if_needed(video_path: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let metadata = tokio::fs::metadata(video_path)
        .await
        .with_context(|| format!("cannot get metadata for {video_path:?}"))?;
    let file_size = metadata.len();

    if file_size <= MAX_FILE_SIZE_BYTES {
        return Ok(vec![video_path.to_path_buf()]);
    }

    let duration_secs = match get_video_duration(video_path).await {
        Ok(d) if d > 1.0 => d,
        Ok(_) => anyhow::bail!("video duration is too short to split"),
        Err(e) => {
            tracing::warn!("could not probe duration for splitting: {e}");
            anyhow::bail!("cannot probe video duration for splitting: {e}");
        }
    };

    let num_parts = (file_size as f64 / MAX_FILE_SIZE_BYTES as f64).ceil() as u32 + 1;
    let num_parts = num_parts.max(2);
    let segment_time = (duration_secs / num_parts as f64).floor().max(1.0);

    let parent_dir = video_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let ext = video_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4");

    let segment_pattern = parent_dir
        .join(format!("{stem}_part%03d.{ext}"))
        .to_str()
        .unwrap()
        .to_string();
    let segment_time_str = format!("{segment_time:.2}");

    tracing::info!(
        "Splitting video {video_path:?} ({file_size} bytes, {duration_secs:.1}s) into {num_parts} parts (segment_time: {segment_time_str}s)"
    );

    let split_args = [
        "-i",
        video_path.to_str().unwrap(),
        "-c",
        "copy",
        "-map",
        "0",
        "-f",
        "segment",
        "-segment_time",
        &segment_time_str,
        "-reset_timestamps",
        "1",
        "-segment_format_options",
        "movflags=+faststart",
        "-y",
        &segment_pattern,
    ];

    run_ffmpeg(&split_args).await?;

    // Collect created part files matching {stem}_part*.{ext}
    let mut parts: Vec<std::path::PathBuf> = Vec::new();
    let mut read_dir = tokio::fs::read_dir(parent_dir).await?;
    let prefix = format!("{stem}_part");
    let extension = format!(".{ext}");

    while let Some(entry) = read_dir.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(&prefix) && name_str.ends_with(&extension) {
            parts.push(entry.path());
        }
    }

    parts.sort();

    if parts.is_empty() {
        anyhow::bail!("ffmpeg segment completed but no split files were produced");
    }

    tracing::info!(
        "Successfully split {video_path:?} into {} files: {parts:?}",
        parts.len()
    );
    Ok(parts)
}
