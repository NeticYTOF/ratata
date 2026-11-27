use ratata::config::state_path;
use ratata::download::DownloadManager;
use ratata::download::direct::single_part_path;
use ratata::download::state::{DownloadState, SegmentState};
use ratata::{Config, DownloadJob, Media, MediaFormat, MediaKind, http};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn payload() -> Vec<u8> {
    let mut data = Vec::with_capacity(2_400_000);
    while data.len() < 2_400_000 {
        data.extend_from_slice(b"ratata-blossom-tunnel-segment-test-");
    }
    data.truncate(2_400_000);
    data
}

async fn serve(payload: Arc<Vec<u8>>, requests: Arc<Mutex<Vec<String>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let payload = payload.clone();
            let requests = requests.clone();
            tokio::spawn(async move { respond(stream, payload, requests).await });
        }
    });
    format!("http://{address}/clip.mp4")
}

async fn respond(mut stream: TcpStream, payload: Arc<Vec<u8>>, requests: Arc<Mutex<Vec<String>>>) {
    let mut buffer = [0u8; 4096];
    let read = stream.read(&mut buffer).await.unwrap_or(0);
    let request = String::from_utf8_lossy(&buffer[..read]);
    let is_head = request.starts_with("HEAD");
    let range = request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("range") {
                Some(value.trim().to_owned())
            } else {
                None
            }
        })
        .unwrap_or_default();
    requests.lock().unwrap().push(range.clone());

    let (start, end) = range
        .strip_prefix("bytes=")
        .and_then(|value| value.split_once('-'))
        .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)))
        .unwrap_or((0, payload.len() - 1));
    let end = end.min(payload.len() - 1);
    let body = &payload[start..=end];
    let status = if range.is_empty() {
        "200 OK"
    } else {
        "206 Partial Content"
    };
    let content_range = if range.is_empty() {
        String::new()
    } else {
        format!("Content-Range: bytes {start}-{end}/{}\r\n", payload.len())
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\n{content_range}Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await.unwrap();
    if !is_head {
        stream.write_all(body).await.unwrap();
    }
    stream.flush().await.unwrap();
}
fn test_setup(url: &str, label: &str) -> (Config, DownloadJob) {
    let config = Config {
        segments: 4,
        min_segment_size: 600_000,
        retries: 2,
        tui: false,
        ..Config::default()
    };
    let mut media = Media::new("test", url, "integration clip");
    let mut format = MediaFormat::new("test", url, MediaKind::Video);
    format.size = Some(2_400_000);
    format.ext = "mp4".into();
    media.add(format.clone());
    let destination = std::env::temp_dir().join(format!(
        "ratata-integration-{}-{label}.mp4",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&destination);
    let _ = std::fs::remove_file(single_part_path(&destination));
    let _ = std::fs::remove_file(state_path(&destination));
    (
        config,
        DownloadJob {
            id: 0,
            media,
            format,
            destination,
            temp: None,
            mp3: None,
            keep_source: false,
            validation_error: None,
        },
    )
}

#[tokio::test]
async fn downloads_all_segments_byte_for_byte() {
    let payload = Arc::new(payload());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let url = serve(payload.clone(), requests.clone()).await;
    let (config, job) = test_setup(&url, "segments");
    let destination = job.destination.clone();
    let client = http::build_client(&config).unwrap();
    let manager = DownloadManager::new(config, client, None);
    manager.download(job).await.unwrap();

    let written = tokio::fs::read(&destination).await.unwrap();
    assert_eq!(written, *payload);
    let ranges = requests.lock().unwrap().clone();
    assert!(
        ranges.iter().any(|r| r.starts_with("bytes=0-")),
        "missing first range: {ranges:?}"
    );
    assert!(
        ranges.iter().any(|r| r.starts_with("bytes=1800000-")),
        "missing last range: {ranges:?}"
    );
    tokio::fs::remove_file(&destination).await.unwrap();
}

#[tokio::test]
async fn resumes_completed_segments_from_state() {
    let payload = Arc::new(payload());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let url = serve(payload.clone(), requests.clone()).await;
    let (config, job) = test_setup(&url, "resume");
    let destination = job.destination.clone();
    let total = 2_400_000u64;
    let chunk = 600_000u64;

    let part = single_part_path(&destination);
    let mut file = std::fs::File::create(&part).unwrap();
    use std::io::Write;
    file.write_all(&payload[..1_200_000]).unwrap();
    file.set_len(total).unwrap();
    drop(file);
    let segments = (0..4)
        .map(|index| {
            let start = index as u64 * chunk;
            SegmentState {
                index,
                start,
                end: start + chunk - 1,
                done: if index < 2 { chunk } else { 0 },
            }
        })
        .collect();
    DownloadState::new(&url, Some(total), None, segments)
        .save(&state_path(&destination))
        .unwrap();

    let client = http::build_client(&config).unwrap();
    let manager = DownloadManager::new(config, client, None);
    manager.download(job).await.unwrap();

    let written = tokio::fs::read(&destination).await.unwrap();
    assert_eq!(written, *payload);
    let ranges = requests.lock().unwrap().clone();
    assert!(
        !ranges.iter().any(|r| r.starts_with("bytes=0-")),
        "re-downloaded a completed segment: {ranges:?}"
    );
    assert!(
        ranges.iter().any(|r| r.starts_with("bytes=1200000-")),
        "missing resume range: {ranges:?}"
    );
    tokio::fs::remove_file(&destination).await.unwrap();
}
