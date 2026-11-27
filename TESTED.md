# Ratata live test results

Machine: Windows (this workstation), 2026-11-27. All runs used the built `ratata` binary
against real public URLs. Status: **works live**, **partial**, or **broken**.

| Platform | Status | URL tested | Result |
| --- | --- | --- | --- |
| HLS (TS, Mux) | works live | https://test-streams.mux.dev/x36xhzz/url_2/193039199_mp4_h264_aac_ld_7.m3u8 | 22,418,060-byte TS, 0x47 sync bytes. Reduced segment concurrency to avoid CDN resets. |
| HLS (fMP4 byte ranges, Apple) | works live | https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/v2/prog_index.m3u8 | 27,672,619-byte MP4, `ftypmp42`. Added `EXT-X-BYTERANGE` parsing. |
| Generic direct MP4 | works live | https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4 | 1,128,375 bytes, `ftypmp42` header. Fixed doubled `.mp4.mp4` filename. |

Notes are appended as platforms are tested. `PROGRESS.md` carries the same summary table.
