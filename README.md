# Ratata ✿

A fast, Rust-native terminal downloader for video, audio and images from social apps.
It downloads through many parallel HTTP ranges into a single preallocated file, resumes
interrupted transfers, retries intelligently, and renders the whole thing as an animated
flower tunnel in your terminal.

Repo: https://github.com/NeticYTOF/ratata

## Install

Requires Rust stable (1.85+).

```sh
git clone https://github.com/NeticYTOF/ratata.git
cd ratata
cargo build --release
./target/release/ratata --help
```

Windows and Linux x86_64 release binaries are attached to GitHub releases.
macOS builds from source with the same commands.

## Use

```sh
# Download with the animated flower-tunnel UI
ratata download "https://www.youtube.com/watch?v=..." \
               "https://www.instagram.com/reel/..."

# Plain progress output (scripts, SSH, no TTY)
ratata download URL --no-tui

# Inspect metadata and available formats
ratata info URL
ratata formats URL

# Pick a format or audio only
ratata download URL --format 18
ratata download URL --audio-only
ratata download URL --all-formats

# Control speed
ratata download URL --segments 12 --retries 6
ratata download URL -o ~/Videos/my-clip.mp4
ratata download URL --cookies ~/cookies.txt

# Configuration
ratata config path
ratata config init
ratata config show
```

Multiple URLs are downloaded concurrently. Existing files are never overwritten;
`ratata download URL --overwrite` replaces them.

## Supported sources

| Source | Notes |
| --- | --- |
| YouTube / youtu.be / Shorts | Innertube player clients, signature deciphering, HLS fallback |
| Instagram posts, reels, TV | Media API with embed fallback, carousels |
| X / Twitter | Syndication API with public mirror fallback |
| TikTok | Page state extraction, image posts |
| Reddit | `.json` media metadata, galleries |
| Vimeo | Public player config, progressive/HLS/DASH |
| Dailymotion | Player metadata, quality variants |
| SoundCloud | Track transcodings with client-id resolution |
| Twitch clips | `videoQualities` from clip pages |
| Streamable | Public video API |
| Imgur / Pinterest | Embed pages, Open Graph media |
| Bilibili | Public view + playurl APIs with Referer |
| Kick | Videos and clips from public API |
| Facebook | Public video embed playable URLs |
| Any direct media URL | mp4, webm, mp3, images, m3u8, mpd |
| Any HTML page | `og:video`, `og:image`, JSON-LD, `<video>`, `<source>`, `<img>` |

Extraction depends on what each site serves publicly without login. Export a Netscape
cookie file from your browser and pass `--cookies FILE` for private media; cookies are
sent only to their matching domains.

## Speed design

- HEAD/range probe discovers size and byte-range support before downloading.
- The download file is preallocated once; each segment has its own handle and writes
  at a fixed offset. There is no second assembly copy.
- Segment count adapts to file size and an exponential moving average of measured
  throughput; `--segments` caps it.
- Retries use exponential backoff with jitter. A failed range resumes from the last
  confirmed byte, and state is persisted next to the output as `*.ratata.json`.
- Connection pooling and keep-alive are enabled for all hosts.
- HLS and DASH manifests choose the highest-bandwidth variant and fetch segments in
  parallel; AES-128 HLS keys and fMP4 init segments are handled. TS playlists are
  remuxed to MP4 when `ffmpeg` is on PATH.

## Terminal UI

- `q` / `Esc`: cancel and quit
- `p` / `Space`: pause or resume all downloads
- `j` / `k` or arrows: select a job
- `t`: cycle themes (`blossom`, `aurora`, `ember`)

The UI enables virtual-terminal processing on Windows consoles through crossterm.

## Config

Config lives in the platform config directory (`dirs::config_dir()/ratata/config.toml`):

```toml
output_dir = "~/Downloads/Ratata"
segments = 8
concurrent_downloads = 3
retries = 4
timeout_secs = 30
min_segment_size = 1048576
theme = "blossom"
tui = true
overwrite = false
```

## Development

```sh
cargo fmt --all
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
```

CI runs format, build, test and clippy on Ubuntu and Windows. Release tags build
Windows and Linux x86_64 binaries.

## License

MIT
