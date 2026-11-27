pub mod dash;
pub mod hls;

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};

pub async fn concat_parts(parts: &[PathBuf], destination: &Path) -> Result<u64> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(destination)
        .await?;
    let mut writer = BufWriter::with_capacity(1 << 20, file);
    let mut total = 0u64;
    for part in parts {
        let mut reader = File::open(part).await?;
        total += tokio::io::copy(&mut reader, &mut writer).await?;
    }
    writer.flush().await?;
    Ok(total)
}

pub fn ffmpeg_available(configured: Option<&str>) -> Option<PathBuf> {
    let mut candidates = configured
        .map(PathBuf::from)
        .into_iter()
        .chain([PathBuf::from("ffmpeg")]);
    candidates.find(|candidate| {
        Command::new(candidate)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

pub fn remux_ts(input: &Path, output: &Path, ffmpeg: &Path) -> Result<()> {
    let status = Command::new(ffmpeg)
        .arg("-y")
        .arg("-i")
        .arg(input)
        .args(["-c", "copy", "-bsf:a", "aac_adtstoasc"])
        .arg(output)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("ffmpeg remux failed"))
    }
}
