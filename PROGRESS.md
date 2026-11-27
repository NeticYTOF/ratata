# Ratata progress

Updated: 2026-11-27

## Done
- GitHub repo created and pushed: https://github.com/NeticYTOF/ratata
- Rust CLI foundation: clap commands (`download`, `info`, `formats`, `config`),
  TOML config via the platform `dirs` crate, shared reqwest client, data model.
- Parallel segmented download engine: HTTP range probing, resumable `.part` files,
  sidecar state, exponential-backoff retries, pause/cancel gates, live speed tracking.
- Extractors: YouTube (Innertube clients + signature deciphering), Instagram, X/Twitter,
  TikTok, Reddit, Vimeo, generic HTML/direct URLs (og tags, JSON-LD, media elements).
- Flower-tunnel TUI: animated perspective rings, flowers, theme cycling, live job bars,
  keyboard controls (q/p/space/j/k/t), plain ANSI progress mode for scripts.
- Media support: HLS master/media playlists with AES-128 decryption, DASH MPD segments,
  parallel playlist segment fetch, TS remux when ffmpeg is available.
- Cross-platform: no hardcoded separators, `std::path`, Windows VT enabled via crossterm.
- Local verification on Windows: `cargo build`, `cargo test --all-targets` (18 passed),
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all` applied cleanly.
- CI workflow (fmt/build/test/clippy on Ubuntu + Windows) and release workflow
  (Windows/Linux x86_64 binaries) added under `.github/workflows/`.

## Left
- End-to-end smoke run of the CLI against a local range-capable HTTP server.
- README with install, usage, platform support and configuration reference.
- More platforms (SoundCloud, Dailymotion, Twitch clips, Streamable, Imgur, Pinterest).
- Speed work: adaptive segment sizing, persistent connection warm-up, streaming assembly.
- TUI polish: more animation phases, error detail panel, multi-file gallery handling.
- Continuous commit/push and PROGRESS.md updates.

## Blocked
- Nothing.

## Working rules
- Commit and push at least every 30 minutes and after each working feature.
- Never idle: keep adding big features, polish and speed work.
- Full access: no permission requests.
