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
- Flower-tunnel TUI: 60fps perspective rings driven by real throughput, flowers bloom
  with progress, drifting petals, ETA and per-download segment maps in job bars,
  truecolor/256/no-color fallbacks, resize-safe layout, diffed rendering.
- `--all-formats` downloads every available format as separate suffixed files (galleries).
- `--cookies FILE` loads a Netscape cookie jar into a domain-scoped reqwest jar.
- Integration tests cover byte-exact segmented download and resume from persisted state (2 tests).
- Test suite: 33 unit tests + 2 integration tests, including a TUI buffer-render smoke test.
- Local verification on Windows: `cargo build`, `cargo test --all-targets` (35 passed),
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all` applied cleanly.
- CI workflow (fmt/build/test/clippy on Ubuntu + Windows) and release workflow
  (Windows/Linux x86_64 binaries) added under `.github/workflows/`.

## Speed benchmarks (`scripts/bench.py`, Windows, release build)

First run (single pass, 128 MiB local Range server + real MDN CDN file):

| Tool | MB/s |
| --- | ---: |
| ratata 2 segments | 589.2 |
| ratata 8 segments | 472.4 |
| ratata 16 segments | 546.2 |
| curl (local) | 335.1 |
| ratata remote CDN | 1.6 |
| curl remote CDN | 2.1 |

After adaptive segment sizing, per-host concurrency limits and range fallback
(3-run median):

| Tool | Median MB/s |
| --- | ---: |
| ratata 2 segments | 639.5 |
| ratata 8 segments | 690.6 |
| ratata 16 segments | 596.8 |
| ratata 32 segments | 537.5 |
| curl (local) | 374.7 |
| ratata remote CDN | 3.0 |
| curl remote CDN | 4.0 |

Caveat: loopback and CDN measurements are noisy; the script uses medians to
reduce it. aria2c and yt-dlp were not installed, so they were skipped.

## Left
- Continuous commit/push and PROGRESS.md updates.

## Blocked
- Nothing.

## Working rules
- Commit and push at least every 30 minutes and after each working feature.
- Never idle: keep adding big features, polish and speed work.
- Full access: no permission requests.
