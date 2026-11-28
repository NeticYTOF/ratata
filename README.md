# RATATA

FAST NATIVE DOWNLOADER

RATATA saves video audio and images from public links
18 named services plus direct media links and generic pages
no account needed
no API key needed

## WHY

built in Rust
parallel segmented downloads
resume after interruption
clean progress screen
private links work with cookies

## INSTALL

download a binary from RELEASES

https://github.com/NeticYTOF/ratata/releases

WINDOWS x86_64 zip
LINUX x86_64 static musl tar.gz

LINUX

    tar xzf ratata-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
    ./ratata --help

FROM SOURCE

    cargo install --git https://github.com/NeticYTOF/ratata

## RUN

    ratata https://www.youtube.com/watch?v=...
    ratata https://www.instagram.com/reel/...
    ratata "https://www.youtube.com/playlist?list=PL..."
    ratata --mp3 https://soundcloud.com/forss/flickermood
    ratata mp3 "https://open.spotify.com/track/..."
    ratata info https://www.youtube.com/watch?v=...

no args opens a prompt
paste a link
answer VIDEO or MP3

PUBLIC PLAYLISTS

YouTube
Spotify
Apple Music

every track queued

KEYS

Esc back to start
q quit
p pause
up down select
summary waits for a key

MP3

--mp3 saves tagged MP3
192 kbps
ID3 tags
cover art
needs ffmpeg

SPOTIFY AND APPLE MUSIC

NOT downloaded from those services
metadata read
audio matched on a public YouTube source
tracks queued one by one

PLAIN MODE

--no-tui
non interactive exits normally

MORE

ratata --help-all

## SUPPORTED

18 named services

YouTube
YouTube Music
Instagram
X Twitter
TikTok
Reddit
Vimeo
Dailymotion
Streamable
Twitch clips
SoundCloud
Imgur
Pinterest
Bilibili
Kick
Facebook
Spotify
Apple Music

plus direct media links
plus generic pages with Open Graph or JSON LD

private posts

    ratata URL --cookies cookies.txt

some sites block datacenter IPs
Reddit often
Bilibili Vimeo SoundCloud sometimes
errors stay short
verbose keeps full detail

## CONFIG

settings in config dir
ratata/config.toml

    ratata config init
    ratata config show

defaults

downloads to ~/Downloads/Ratata
8 segments
3 concurrent downloads
4 retries

flags

--output
--segments
--retries
--overwrite

## DEVELOP

    cargo test --all-targets
    cargo clippy --all-targets -- -D warnings
    python scripts/bench.py --ratata target/release/ratata

CI builds on Windows and Linux when Actions is enabled

## LICENSE

MIT
