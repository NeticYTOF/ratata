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
