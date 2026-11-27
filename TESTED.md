# Ratata live test results

Machine: Windows (this workstation), 2026-11-27. All runs used the built `ratata` binary
against real public URLs. Status: **works live**, **partial**, or **broken**.

| Platform | Status | URL tested | Result |
| --- | --- | --- | --- |
| HLS (TS, Mux) | works live | https://test-streams.mux.dev/x36xhzz/url_2/193039199_mp4_h264_aac_ld_7.m3u8 | 22,418,060-byte TS, 0x47 sync bytes. Reduced segment concurrency to avoid CDN resets. |
| DASH (time timeline) | works live | https://demo.unified-streaming.com/k8s/features/stable/video/tears-of-steel/tears-of-steel.ism/tears-of-steel.mpd | 192,110,235-byte MP4, `ftypiso6`. Fixed per-AdaptationSet templates and `$Time$` timelines. |
| X / Twitter | works live | https://twitter.com/pdxdylan/status/1540398733669666818 | 229,533-byte MP4 with `ftyp`. Fixed filename sanitization for titles containing URLs. |
| Generic direct MP4 | works live | https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4 | 1,128,375 bytes, `ftypmp42` header. Fixed doubled `.mp4.mp4` filename. |

Notes are appended as platforms are tested. `PROGRESS.md` carries the same summary table.
