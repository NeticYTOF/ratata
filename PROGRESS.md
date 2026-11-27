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
- Minimal CLI-style TUI: block logo, rounded panels, aligned rows with platform badge,
  title, size, percent, sub-cell progress bar, measured speed and ETA; summary panel
  stays open until a key is pressed; diffed rendering; truecolor/256/16/no-color support.
- Speed readout fixed: rates are measured from byte deltas, not reporter ticks.
- Friendly CLI: `ratata URL` downloads with sensible defaults, prompts for a link when
  run with no arguments on a TTY, plain-language errors with suggested fixes, expert
  flags hidden behind `--help-all`.
- `--all-formats` downloads every available format as separate suffixed files (galleries).
- `--cookies FILE` loads a Netscape cookie jar into a domain-scoped reqwest jar.
- Integration tests cover byte-exact segmented download and resume from persisted state (2 tests).
- Test suite: 49 offline tests pass (46 library, 1 binary, 2 integration).
- Local verification on Windows: `cargo build`, `cargo test --all-targets`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`.
- v0.1.0 released at https://github.com/NeticYTOF/ratata/releases/tag/v0.1.0 with
  Windows x86_64 zip, Linux x86_64 tar.gz and SHA256SUMS.
- TUI batch fixes: MP3 jobs finish on final `.mp3` path with intermediate and sidecar cleanup,
  per-job temp/final paths avoid collisions, invalid links become friendly failed rows,
- Summary screen now shows friendly failure reasons on failed rows and hides the saved-folder line when zero files were saved.

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
- External Linux re-test of the fixes listed in `TESTED.md`.
- GitHub Actions CI/release runs once Actions is enabled for the account.

## Blocked
- GitHub Actions: `gh workflow run` returned "Actions has been disabled for this user";
  workflows remain in the repo and will run once Actions is enabled.
- Datacenter-IP blocks documented in `TESTED.md`: Reddit (403), Bilibili (412),
  Vimeo (CDN no progress), SoundCloud (no playable transcoding), Dailymotion CDN (403).

## Working rules
- Commit and push at least every 30 minutes and after each working feature.
- Never idle: keep adding big features, polish and speed work.
- Full access: no permission requests.
