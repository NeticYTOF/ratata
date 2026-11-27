# Ratata progress

Updated: 2026-11-27

## Done
- GitHub repo created and pushed: https://github.com/NeticYTOF/ratata
- Rust CLI foundation: clap commands (`download`, `info`, `formats`, `config`),
  TOML config via the platform `dirs` crate, shared reqwest client, data model.
- Parallel segmented download engine: HTTP range probing, resumable preallocated `.part`
  file with concurrent offset writes, sidecar state, retries, pause/cancel, speed tracking.
- Adaptive segment sizing from observed throughput EMA; no second copy/assembly pass.
- Extractors: YouTube (Innertube + signature deciphering), Instagram, X/Twitter, TikTok,
  Reddit, Vimeo, Dailymotion, SoundCloud, Twitch, Streamable, Imgur, Pinterest, Bilibili,
  Kick, Facebook, generic HTML/direct URLs (og tags, JSON-LD, media elements).
- Flower-tunnel TUI: perspective rings, flowers, drifting petals, breathing bloom,
  theme cycling, live job bars, error detail panel, keyboard controls (q/p/space/j/k/t).
- `--all-formats` downloads every available format as separate suffixed files (galleries).
- `--cookies FILE` loads a Netscape cookie jar into a domain-scoped reqwest jar.
- Integration tests cover byte-exact segmented download and resume from persisted state (2 tests).
- Test suite: 28 unit tests + 2 integration tests passing locally.
- Local verification on Windows: `cargo build`, `cargo test --all-targets` (30 passed),
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all` applied cleanly.
- CI workflow (fmt/build/test/clippy on Ubuntu + Windows) and release workflow
  (Windows/Linux x86_64 binaries) added under `.github/workflows/`.

## Left
- Continuous commit/push and PROGRESS.md updates.

## Blocked
- Nothing.

## Working rules
- Commit and push at least every 30 minutes and after each working feature.
- Never idle: keep adding big features, polish and speed work.
- Full access: no permission requests.
