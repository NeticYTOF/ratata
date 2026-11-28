# Ratata

Download videos, audio and images from social sites — fast, simple, and native.
Ratata uses parallel connections, resumes interrupted downloads, and shows a clean
progress screen. No account or API key needed.

## Install

Download a ready-made binary from [Releases](https://github.com/NeticYTOF/ratata/releases)
(Windows x86_64 zip, Linux x86_64 static musl tar.gz — works on older glibc distros).
On Linux: `tar xzf ratata-v0.1.0-x86_64-unknown-linux-gnu.tar.gz && ./ratata --help`
(the tar file preserves the executable bit). Or install from source with Rust:

```sh
cargo install --git https://github.com/NeticYTOF/ratata
```

## Use

```sh
ratata https://www.youtube.com/watch?v=...
ratata https://www.instagram.com/reel/... "https://vimeo.com/..."
ratata "https://www.youtube.com/playlist?list=PL..."
ratata --mp3 https://soundcloud.com/forss/flickermood
ratata mp3 "https://open.spotify.com/track/..."
ratata info https://www.youtube.com/watch?v=...
```

Paste a link and Ratata saves the best version to your Downloads folder. With no
arguments in a terminal it asks for a link and whether you want Video or MP3.
Public YouTube playlist links queue every video; several URLs can be pasted in
one command or added later after pressing `Esc`.

- `--mp3` saves tagged MP3 (192 kbps, ID3 tags and cover art). Needs `ffmpeg`.
- Spotify and Apple Music links are **not downloaded from those services**. Ratata
  reads track metadata, finds the matching audio on a public YouTube source, and
  saves that. Albums/playlists are processed track by track.
- YouTube Music links prefer the best audio track.
- `--no-tui` gives plain output for scripts; non-interactive use exits automatically.
- Press `Esc` to go back to the start, `q` to quit, `p` to pause, `↑`/`↓` to select.
  The final summary stays open until you press a key.

Run `ratata --help-all` for advanced options (format, cookies, segment counts).

## Supported sites

YouTube, YouTube Music, Instagram, X/Twitter, TikTok, Reddit, Vimeo, Dailymotion,
Streamable, Twitch clips, SoundCloud, Imgur, Pinterest, Bilibili, Kick, Facebook,
Spotify and Apple Music metadata matching, direct media links and HTML pages with
Open Graph or JSON-LD media. Private posts need a Netscape cookie file:
`ratata URL --cookies cookies.txt`.

Reddit blocks many datacenter IPs; Ratata tries `www`, `api`, and `old` endpoints.
Some sites (notably Reddit and sometimes Bilibili, Vimeo or SoundCloud) block
datacenter IPs with HTTP 403/412 even when the same link works at home. Ratata
reports the real error and suggests cookies when that happens.

## Config

Settings live in your platform config directory (`dirs::config_dir()/ratata/config.toml`):

```sh
ratata config init
ratata config show
```

Defaults: downloads to `~/Downloads/Ratata`, 8 segments, 3 concurrent downloads,
4 retries. Override with `--output`, `--segments`, `--retries`, `--overwrite`.

## Development

```sh
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
python scripts/bench.py --ratata target/release/ratata
```

CI builds and tests on Windows and Linux once GitHub Actions is enabled.

## License

MIT
