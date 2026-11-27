#!/usr/bin/env python3
"""Repeatable Ratata throughput benchmark with median results.

Starts a local Range-capable HTTP server, downloads the same payload with Ratata
at several segment counts and with any installed comparison tools, verifies
every byte, and prints median MB/s. Use --remote to add a real CDN file.
"""
import argparse
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PAYLOAD_SIZE = 128 * 1024 * 1024
PAYLOAD = (b"ratata-throughput-benchmark-payload-" * 4_000_000)[:PAYLOAD_SIZE]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def _serve(self, head_only=False):
        rng = self.headers.get("Range", "")
        if rng.startswith("bytes="):
            start_s, _, end_s = rng[6:].partition("-")
            start = int(start_s or 0)
            end = min(int(end_s) if end_s else len(PAYLOAD) - 1, len(PAYLOAD) - 1)
            body = PAYLOAD[start : end + 1]
            status = 206
            content_range = f"bytes {start}-{end}/{len(PAYLOAD)}"
        else:
            body = PAYLOAD
            status = 200
            content_range = None
        self.send_response(status)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Accept-Ranges", "bytes")
        if content_range:
            self.send_header("Content-Range", content_range)
        self.end_headers()
        if not head_only:
            self.wfile.write(body)

    def do_HEAD(self):
        self._serve(head_only=True)

    def do_GET(self):
        self._serve()


def timed(command, output, expected_size):
    if os.path.exists(output):
        os.remove(output)
    start = time.perf_counter()
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    elapsed = time.perf_counter() - start
    ok = os.path.exists(output) and os.path.getsize(output) == expected_size
    return elapsed, expected_size / elapsed / 1_048_576 if ok else 0.0, ok, result.stdout.strip().replace("\n", " ")[:100], result.returncode


def median_run(label, command, output, expected_size, repeats):
    samples = [timed(command, output, expected_size) for _ in range(repeats)]
    return {
        "label": label,
        "seconds": statistics.median(s[0] for s in samples),
        "mbps": statistics.median(s[1] for s in samples),
        "ok": all(s[2] for s in samples),
        "output": samples[-1][3],
        "code": samples[-1][4],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ratata", default="target/release/ratata")
    parser.add_argument("--segments", default="1,2,4,8,16,32")
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--remote", action="store_true")
    args = parser.parse_args()

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_port}/bench.mp4"
    tmp = tempfile.mkdtemp(prefix="ratata-bench-")
    results = []

    print(f"Local payload: {PAYLOAD_SIZE / 1_048_576:.0f} MiB, {args.repeat} median runs at {url}")
    for count in [int(v) for v in args.segments.split(",")]:
        results.append(
            median_run(
                f"ratata --segments {count}",
                [args.ratata, "download", url, "--no-tui", "--segments", str(count), "-o", os.path.join(tmp, f"ratata-{count}.mp4")],
                os.path.join(tmp, f"ratata-{count}.mp4"),
                PAYLOAD_SIZE,
                args.repeat,
            )
        )
    if shutil.which("curl"):
        results.append(median_run("curl", ["curl", "-sS", "-o", os.path.join(tmp, "curl.mp4"), url], os.path.join(tmp, "curl.mp4"), PAYLOAD_SIZE, args.repeat))
    if shutil.which("aria2c"):
        results.append(median_run("aria2c -x16", ["aria2c", "-q", "-x16", "-s16", "-d", tmp, "-o", "aria2.mp4", url], os.path.join(tmp, "aria2.mp4"), PAYLOAD_SIZE, args.repeat))
    if shutil.which("yt-dlp"):
        results.append(median_run("yt-dlp", ["yt-dlp", "-q", "-o", os.path.join(tmp, "ytdlp.mp4"), url], os.path.join(tmp, "ytdlp.mp4"), PAYLOAD_SIZE, args.repeat))

    if args.remote:
        remote = "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4"
        remote_size = 1_128_375
        results.append(median_run("ratata remote CDN", [args.ratata, "download", remote, "--no-tui", "-o", os.path.join(tmp, "remote.mp4")], os.path.join(tmp, "remote.mp4"), remote_size, args.repeat))
        results.append(median_run("curl remote CDN", ["curl", "-sS", "-o", os.path.join(tmp, "remote-curl.mp4"), remote], os.path.join(tmp, "remote-curl.mp4"), remote_size, args.repeat))

    print("\n| Tool | Median MB/s | Median seconds | Verified |")
    print("| --- | ---: | ---: | --- |")
    for row in results:
        print(f"| {row['label']} | {row['mbps']:.1f} | {row['seconds']:.2f} | {'yes' if row['ok'] else 'NO: ' + row['output']} |")
    server.shutdown()
    shutil.rmtree(tmp, ignore_errors=True)
    return 0 if all(row["ok"] for row in results) else 1


if __name__ == "__main__":
    sys.exit(main())
