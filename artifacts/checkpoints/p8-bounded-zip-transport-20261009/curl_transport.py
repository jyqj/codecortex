"""Bounded read-only curl adapter for a fixed HTTPS artifact.
No automatic redirects, retries, local files, raw headers, URL logging, or product calls.
Caller must verify the complete registered SHA and every later range block.
Use as a context manager or close it explicitly so an abandoned generator also stops its owned curl.
"""
import re
import subprocess
import time
from urllib.parse import urlsplit

BLOCK = 256 * 1024
MAX_HEADER_BYTES = 64 * 1024
MAX_HEADER_LINE = 8192

class TransferError(RuntimeError):
    pass

def require(ok, message):
    if not ok:
        raise TransferError(message)

class CurlTransport:
    def __init__(self, url, size, allowed_host, seconds=1800):
        require(isinstance(url, str) and not any(ord(c) <= 32 or ord(c) == 127 for c in url),
                "invalid artifact URL syntax")
        try:
            parts = urlsplit(url)
            port = parts.port
        except (ValueError, TypeError):
            raise TransferError("invalid artifact URL authority") from None
        require(parts.scheme == "https" and parts.hostname == allowed_host
                and parts.username is None and parts.password is None
                and port in (None, 443) and not parts.fragment,
                "artifact URL outside fixed HTTPS origin")
        require(isinstance(size, int) and not isinstance(size, bool) and size > 0,
                "invalid registered size")
        require(isinstance(seconds, (int, float)) and 0 < seconds <= 1800,
                "invalid transport deadline")
        self._url = url
        self.size = size
        self.deadline = time.monotonic() + seconds
        self.attempted_requests = 0
        self.completed_requests = 0
        self.received_body_bytes = 0
        self.received_header_bytes = 0
        self._origin_used = False
        self._owned = set()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        self.close()

    def close(self):
        for process in tuple(self._owned):
            self._stop(process)

    def _request(self, maximum, byte_range=None, etag=None):
        require(not self._owned, "concurrent curl processes are forbidden")
        remaining = self.deadline - time.monotonic()
        require(remaining > 0, "transport deadline exceeded")
        require(0 < maximum <= self.size, "response allocation bound invalid")
        argv = ["curl", "--fail", "--silent", "--no-buffer", "--http1.1",
                "--suppress-connect-headers", "--proto", "=https",
                "--proto-redir", "=https", "--max-redirs", "0",
                "--retry", "0", "--connect-timeout", str(min(30, remaining)),
                "--max-time", str(min(240, remaining)), "--max-filesize", str(maximum),
                "--header", "Accept-Encoding: identity", "--dump-header", "-"]
        if byte_range is not None:
            start, end = byte_range
            require(isinstance(start, int) and not isinstance(start, bool)
                    and isinstance(end, int) and not isinstance(end, bool)
                    and 0 <= start <= end < self.size and end - start + 1 <= BLOCK,
                    "invalid bounded byte range")
            require(isinstance(etag, str) and len(etag) <= 256
                    and re.fullmatch(r'"[^\x00-\x20\x7f"]+"', etag) is not None,
                    "invalid strong ETag")
            argv.extend(["--range", "%d-%d" % (start, end),
                         "--header", "If-Match: " + etag])
        argv.append(self._url)
        self.attempted_requests += 1
        try:
            process = subprocess.Popen(argv, stdin=subprocess.DEVNULL,
                                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            self._owned.add(process)
            return process
        except (OSError, ValueError):
            raise TransferError("curl process could not be started") from None

    def _headers(self, process):
        total = 0
        for _ in range(5):
            line = process.stdout.readline(MAX_HEADER_LINE + 1)
            total += len(line)
            self.received_header_bytes += len(line)
            require(len(line) <= MAX_HEADER_LINE and total <= MAX_HEADER_BYTES,
                    "HTTP header budget exceeded")
            match = re.fullmatch(rb"HTTP/1\.[01] ([0-9]{3})(?: [^\r\n]*)?\r\n", line)
            require(match is not None, "invalid HTTP status framing")
            status = int(match.group(1))
            pairs = []
            while True:
                line = process.stdout.readline(MAX_HEADER_LINE + 1)
                total += len(line)
                self.received_header_bytes += len(line)
                require(len(line) <= MAX_HEADER_LINE and total <= MAX_HEADER_BYTES,
                        "HTTP header budget exceeded")
                require(line.endswith(b"\r\n"), "incomplete HTTP header framing")
                if line == b"\r\n":
                    break
                require(not line.startswith((b" ", b"\t")) and b":" in line,
                        "folded or malformed HTTP header")
                name, value = line[:-2].split(b":", 1)
                require(re.fullmatch(rb"[!#$%&'*+.^_\x60|~0-9A-Za-z-]+", name) is not None,
                        "invalid HTTP header name")
                value = value.strip(b" \t")
                require(not any(c < 32 and c != 9 or c == 127 for c in value),
                        "invalid HTTP header value")
                pairs.append((name.decode("ascii"), value.decode("latin-1")))
            if 100 <= status < 200:
                require(status != 101, "protocol switching is forbidden")
                continue
            return status, pairs
        raise TransferError("too many interim HTTP responses")

    def _read(self, process, count):
        require(0 <= count <= BLOCK + 1, "body read exceeds fixed allocation")
        try:
            data = process.stdout.read(count)
        except OSError:
            raise TransferError("artifact body read failed") from None
        self.received_body_bytes += len(data)
        return data

    def _finish(self, process):
        try:
            code = process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            raise TransferError("curl did not close after response EOF") from None
        require(code == 0, "curl returned a nonzero transport status")
        self.completed_requests += 1

    def _stop(self, process):
        if process is None:
            return
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    raise TransferError("owned curl shutdown not confirmed") from None
        if process.stdout is not None:
            process.stdout.close()
        self._owned.discard(process)

    def origin(self):
        """Return (chunks, status_getter, headers_getter); consume chunks once."""
        require(not self._origin_used, "origin scan cannot be repeated")
        self._origin_used = True
        state = {"status": None, "headers": None, "completed": False}
        def chunks():
            process = None
            try:
                process = self._request(self.size)
                status, pairs = self._headers(process)
                state["status"], state["headers"] = status, pairs
                require(status == 200, "origin response must be HTTP 200")
                received = 0
                while received < self.size:
                    data = self._read(process, min(BLOCK, self.size - received))
                    require(data, "origin response was shorter than registered size")
                    received += len(data)
                    yield data
                require(not self._read(process, 1), "origin response exceeds registered size")
                self._finish(process)
                state["completed"] = True
            finally:
                self._stop(process)
        def status():
            require(state["completed"], "origin scan did not complete")
            return state["status"]
        def headers():
            require(state["completed"], "origin scan did not complete")
            return state["headers"]
        return chunks(), status, headers

    def fetch(self, start, end, etag):
        """Fetch one inclusive range; caller validates all returned header fields."""
        process = None
        try:
            expected = end - start + 1
            process = self._request(expected, (start, end), etag)
            status, pairs = self._headers(process)
            require(status == 206, "range response must be HTTP 206")
            data = self._read(process, expected + 1)
            require(len(data) == expected, "range response length mismatch")
            self._finish(process)
            return status, pairs, data
        finally:
            self._stop(process)

    def safe_metrics(self):
        return {"attempted_requests": self.attempted_requests,
                "completed_requests": self.completed_requests,
                "received_body_bytes": self.received_body_bytes,
                "received_header_bytes": self.received_header_bytes,
                "adapter_writes_files": False,
                "adapter_logs_URL_or_raw_headers": False,
                "automatic_redirects": False, "retries": 0}
