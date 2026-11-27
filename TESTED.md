# Ratata live test results

Machine: Windows (this workstation), 2026-11-27. All runs used the built `ratata` binary
against real public URLs. Status: **works live**, **partial**, or **broken**.

| Platform | Status | URL tested | Result |
| --- | --- | --- | --- |
| HLS (TS, Mux) | works live | https://test-streams.mux.dev/x36xhzz/url_2/193039199_mp4_h264_aac_ld_7.m3u8 | 22,418,060-byte TS, 0x47 sync bytes. Reduced segment concurrency to avoid CDN resets. |
| HLS (fMP4 byte ranges) | works live | https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/v2/prog_index.m3u8 | 27,672,619-byte MP4, `ftypmp42`; `EXT-X-BYTERANGE` support added. |
| YouTube normal video | works live | https://www.youtube.com/watch?v=dQw4w9WgXcQ | 2,058,142-byte format 160 MP4, `ftypdash`. |
| YouTube Shorts | works live | https://www.youtube.com/shorts/lR0Go54CNkE | 274,435-byte MP4, `ftypdash`. |
| YouTube playlist item | works live | https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL6gx4Cwl9DGAAf2ebRn2R4rEfhbN9HWS | Metadata resolved from `v` parameter. |
| YouTube age-neutral | works live | https://www.youtube.com/watch?v=aqz-KE-bpKQ | Big Buck Bunny formats resolved. |
| Reddit | broken from this machine | https://www.reddit.com/r/videos/comments/8l0bfp/the_way_vreddit_was_meant_to_be_experienced/ | All endpoints return HTTP 403 from this IP (www, api, old, RSS), so no media could be fetched. Added www/api/old fallbacks for other networks. |
| Instagram reel | works live | https://www.instagram.com/reel/DXHCQPViRaH | 9,385,649-byte MP4, `ftypisom`. Fixed double-escaped `video_url` in embed HTML and bot-check headers. |
| Instagram post | works live | https://www.instagram.com/p/C8t7g-BAg5g | Video and image formats resolved from embed. |
| TikTok | broken from this machine | https://www.tiktok.com/@123gohouse/video/7070514104485547265 | TikTok geo-redirects this IP (HTTP 302 to `/in/about`) for page, oEmbed and embed endpoints; no playable data reachable. oEmbed fallback is retained. |
| DASH (time timeline) | works live | https://demo.unified-streaming.com/k8s/features/stable/video/tears-of-steel/tears-of-steel.ism/tears-of-steel.mpd | 192,110,235-byte MP4, `ftypiso6`. Fixed per-AdaptationSet templates and `$Time$` timelines. |
| Vimeo | partial | https://vimeo.com/76979871 | Metadata and nested CDN manifest discovery work; media is SAMPLE-AES/FairPlay DRM (`skd://drm`), so clear download is impossible without a license. Fixed nested `cdns` URL parsing. |
| X / Twitter | works live | https://twitter.com/pdxdylan/status/1540398733669666818 | 229,533-byte MP4 with `ftyp`. Fixed filename sanitization for titles containing URLs. |
| Generic direct MP4 | works live | https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4 | 1,128,375 bytes, `ftypmp42` header. Fixed doubled `.mp4.mp4` filename. |

Notes are appended as platforms are tested. `PROGRESS.md` carries the same summary table.

## External Linux run (US datacenter IP, clean clone at 0199ed0)

Build and tests passed (36 unit + 2 integration). Reported failures and current fixes:

| Platform | Status | URL tested | Result / fix |
| --- | --- | --- | --- |
| Streamable | fixed, awaiting retest | https://streamable.com/moo | Probe failure; HEAD/GET probe now times out and falls back to a normal stream, with browser headers on CDN requests. |
| Vimeo | fixed, awaiting retest | https://vimeo.com/76979871 | Hang at 0% with 42.5 MiB known size; probe now bounded by config timeout and falls back to streaming GET. DRM note retained separately. |
| TikTok | fixed, awaiting retest | https://www.tiktok.com/@scout2015/video/6718335390845095173 | CDN 403; formats now carry the page User-Agent, Referer and cookies from the reqwest jar. |
| SoundCloud | fixed, awaiting retest | https://soundcloud.com/forss/flickermood | Added homepage client-id discovery and API resolve fallback. |
| Bilibili | fixed, awaiting retest | https://www.bilibili.com/video/BV1GJ411x7h7 | BV id now searched across all path segments and `bvid` query parameter. |
| Dailymotion | fixed, awaiting retest | https://www.dailymotion.com/video/x2jvvep | Added recursive media URL discovery in metadata and HTML page fallback. |
| Facebook | fixed, awaiting retest | https://www.facebook.com/facebook/videos/10153231379946729/ | Added double-escaped playable URL parsing and `.mp4` fallback scan. |
| Kick | fixed, awaiting retest | https://kick.com/video/example | API 403 from Cloudflare; added browser headers and page fallback. |
| Twitch clips | needs fresh URL | previous slug was dead | Fallback embed parsing retained; a current public clip is needed for verification. |
| Reddit | broken from datacenter IP | https://www.reddit.com/r/videos/comments/8l0bfp/ | Expected 403; no action beyond www/api/old fallbacks. |
| Imgur / Pinterest | fixed, awaiting retest | gallery / pin from Linux run | Pinterest thumbnail sizes are upgraded to `/originals/`; Imgur uses embed media URLs. |
| UI leftovers | fixed | — | Flower glyphs and wording removed from progress and help; summary stays open until a key press on a TTY. |

## External Linux re-test at ca2d55b and fixes in ab70ed3

| Platform | Status | URL tested | Result |
| --- | --- | --- | --- |
| Streamable | works live | https://streamable.com/moo | 2.9 MiB MP4 saved after normalising `https//` CDN URLs. |
| Facebook | works live | https://www.facebook.com/facebook/videos/10153231379946729/ | 1.1 MiB MP4 saved after escaped playable-URL parsing. |
| Vimeo | fixed, awaiting retest | https://vimeo.com/76979871 | Bounded playlist fetches and socket stalls; combined underlying errors are now printed after all format attempts fail. |
| TikTok | fixed, awaiting retest | https://www.tiktok.com/@scout2015/video/6718335390845095173 | Header values are sanitised (unit test added); CDN falls back across all available play URLs on 401/403. |
| SoundCloud | fixed, awaiting retest | https://soundcloud.com/forss/flickermood | Client id now also discovered from homepage JS bundles. |
| Bilibili | fixed, awaiting retest | https://www.bilibili.com/video/BV1GJ411x7h7 | Fresh buvid3/buvid4 cookies, browser headers and `__playinfo__` fallback. |
| Dailymotion | fixed, awaiting retest | https://www.dailymotion.com/video/x2jvvep | Player metadata with exact Referer plus escaped-URL embed fallback. |
| YouTube Music | added, awaiting live test | https://music.youtube.com/watch?v=... | Audio preferred; metadata uses YouTube video details. |
| Spotify / Apple Music | added, awaiting live test | open.spotify.com / music.apple.com links | Metadata resolved without login, matched to public YouTube audio with score-based matching. UI/README disclose the public source. |
| MP3 mode | added, awaiting live test | any URL with `--mp3` | ffmpeg-based 192 kbps MP3 with ID3 tags and optional cover art; friendly error when ffmpeg is absent. |

## Final status before v0.1.0 (Linux re-tests at 0e0cbcb / e420812 / 686404d)

| Item | Status | Notes |
| --- | --- | --- |
| Spotify / Apple Music to MP3 | works live | Metadata resolved, matched on public YouTube source, clean title, cover art and known tags written. |
| YouTube Music to MP3 | works live | Audio selected and converted; no invented album/year/track tags. |
| TikTok | works live | Valid MP4 after fresh cookie jar, Referer/UA and alternate play URLs. |
| Dailymotion | partial | HLS variant is now routed through the playlist downloader with browser headers, but `cdndirector` returns 403 from the datacenter IP. |
| Vimeo | blocked from datacenter IP | Player config is read and all CDN variants are tried; `vod-adaptive-ak.vimeocdn.com` sends no bytes. Ratata now reports a 20-second stall with the URL. |
| SoundCloud | blocked from datacenter IP | SoundCloud-only client-id/resolve path retained; API does not yield a playable transcoding from this IP. No YouTube fallback is used. |
| Bilibili | blocked from datacenter IP | Full browser headers, buvid3/buvid4 from `finger/spi`, page priming and `__playinfo__` fallback still receive HTTP 412. |
| Reddit | blocked from datacenter IP | Expected 403; fallbacks retained. |
| TUI | verified offline | Rendering tests at 110 and 80 columns; summary stays open on TTY and non-TTY exits normally. |
| Release binaries | verified | v0.1.0: Windows x86_64 zip (`--version`/`--help` OK); Linux static musl PIE (`file` verified), refreshed SHA256SUMS uploaded. |
