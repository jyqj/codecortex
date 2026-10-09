"""Pure-memory transport controls: fake curl processes only, no network or files."""
import io
import subprocess
import traceback
import unittest
from unittest.mock import patch
from curl_transport import BLOCK, CurlTransport, TransferError

HOST = "sdmntprwestus.oaiusercontent.com"
URL = "https://" + HOST + "/fixture.zip?capability=PRIVATE_FIXTURE_TOKEN"
TOKEN = "PRIVATE_FIXTURE_TOKEN"
ETAG = '"fixed-fixture-etag"'

def wire(status, body, fields=None):
    pairs = fields if fields is not None else [
        ("Content-Length", str(len(body))), ("ETag", ETAG),
        ("Content-Encoding", "identity")]
    head = ("HTTP/1.1 %d Fixture\r\n" % status).encode("ascii")
    head += b"".join((name + ": " + value + "\r\n").encode("latin-1")
                     for name, value in pairs)
    return head + b"\r\n" + body

class FakeProcess:
    def __init__(self, content, code=0, waits=None):
        self.stdout = io.BytesIO(content)
        self.code = code
        self.returncode = None
        self.waits = list(waits or [])
        self.terminated = 0
        self.killed = 0
    def wait(self, timeout=None):
        if self.waits:
            result = self.waits.pop(0)
            if isinstance(result, BaseException):
                raise result
            self.returncode = result
        else:
            self.returncode = -9 if self.killed else -15 if self.terminated else self.code
        return self.returncode
    def poll(self):
        return self.returncode
    def terminate(self):
        self.terminated += 1
    def kill(self):
        self.killed += 1

class CurlTransportControls(unittest.TestCase):
    def assert_safe_error(self, exc):
        formatted = "".join(traceback.format_exception(type(exc), exc, exc.__traceback__))
        self.assertNotIn(TOKEN, str(exc))
        self.assertNotIn(TOKEN, formatted)
        self.assertNotIn(URL, formatted)

    def test_origin_complete_bounded_chunks_exact_headers_and_metrics(self):
        body = b"x" * (BLOCK * 2 + 31)
        fields = [("Content-Length", str(len(body))), ("ETag", ETAG),
                  ("X-Receipt", "one"), ("X-Receipt", "two")]
        content = wire(200, body, fields)
        process = FakeProcess(content)
        with patch("curl_transport.subprocess.Popen", return_value=process) as launch:
            with CurlTransport(URL, len(body), HOST) as transport:
                chunks, status, headers = transport.origin()
                received = list(chunks)
                self.assertEqual(b"".join(received), body)
                self.assertTrue(all(0 < len(x) <= BLOCK for x in received))
                self.assertEqual(status(), 200)
                self.assertEqual(headers(), fields)
                metrics = transport.safe_metrics()
                self.assertEqual(metrics["received_body_bytes"], len(body))
                self.assertEqual(metrics["received_header_bytes"], len(content) - len(body))
                self.assertEqual(metrics["attempted_requests"], 1)
                self.assertEqual(metrics["completed_requests"], 1)
                args = launch.call_args.args[0]
                self.assertNotIn("--location", args)
                self.assertEqual(args[args.index("--retry") + 1], "0")
                self.assertEqual(args[args.index("--max-redirs") + 1], "0")
                self.assertEqual(launch.call_args.kwargs["stderr"], subprocess.DEVNULL)
            self.assertTrue(process.stdout.closed)
            self.assertEqual(process.terminated, 0)

    def test_range_exact_inclusive_bounds_if_match_and_full_response(self):
        size = 3 * BLOCK
        start, end = BLOCK, 2 * BLOCK - 1
        body = b"r" * BLOCK
        fields = [("Content-Length", str(BLOCK)), ("ETag", ETAG),
                  ("Content-Range", "bytes %d-%d/%d" % (start, end, size))]
        process = FakeProcess(wire(206, body, fields))
        with patch("curl_transport.subprocess.Popen", return_value=process) as launch:
            with CurlTransport(URL, size, HOST) as transport:
                self.assertEqual(transport.fetch(start, end, ETAG), (206, fields, body))
                args = launch.call_args.args[0]
                self.assertEqual(args[args.index("--range") + 1], "%d-%d" % (start, end))
                self.assertIn("If-Match: " + ETAG, args)
                self.assertEqual(transport.received_body_bytes, BLOCK)
                self.assertEqual(transport.completed_requests, 1)
            self.assertTrue(process.stdout.closed)

    def test_origin_short_and_extra_are_rejected_and_reaped(self):
        for actual in (9, 11):
            with self.subTest(actual=actual):
                process = FakeProcess(wire(200, b"x" * actual))
                with patch("curl_transport.subprocess.Popen", return_value=process):
                    with CurlTransport(URL, 10, HOST) as transport:
                        chunks, status, headers = transport.origin()
                        with self.assertRaises(TransferError) as caught:
                            list(chunks)
                        self.assert_safe_error(caught.exception)
                        self.assertEqual(transport.completed_requests, 0)
                        with self.assertRaises(TransferError):
                            status()
                        self.assertEqual(len(transport._owned), 0)
                    self.assertTrue(process.stdout.closed)
                    self.assertEqual(process.terminated, 1)

    def test_range_wrong_status_short_and_extra_are_rejected(self):
        for status, actual in ((200, 10), (206, 9), (206, 11)):
            with self.subTest(status=status, actual=actual):
                process = FakeProcess(wire(status, b"x" * actual))
                with patch("curl_transport.subprocess.Popen", return_value=process):
                    with CurlTransport(URL, 100, HOST) as transport:
                        with self.assertRaises(TransferError) as caught:
                            transport.fetch(0, 9, ETAG)
                        self.assert_safe_error(caught.exception)
                        self.assertEqual(transport.completed_requests, 0)
                        self.assertEqual(len(transport._owned), 0)
                    self.assertTrue(process.stdout.closed)

    def test_invalid_origin_authority_never_starts_curl(self):
        bad = [
            "http://" + HOST + "/fixture.zip",
            "https://unapproved.example/fixture.zip",
            "https://user:password@" + HOST + "/fixture.zip",
            "https://" + HOST + ":444/fixture.zip",
            "https://" + HOST + "/fixture.zip#fragment",
            URL + "\nX-Injected: bad"]
        with patch("curl_transport.subprocess.Popen") as launch:
            for url in bad:
                with self.subTest(case=bad.index(url)):
                    with self.assertRaises(TransferError) as caught:
                        CurlTransport(url, 100, HOST)
                    self.assert_safe_error(caught.exception)
            launch.assert_not_called()

    def test_invalid_range_and_etag_never_start_curl(self):
        bad = [(-1, 2, ETAG), (0, 100, ETAG),
               (0, BLOCK, ETAG), (0, 9, "W/" + ETAG),
               (0, 9, ETAG + "\r\nAuthorization: injected"),
               (0, 9, "unquoted")]
        with patch("curl_transport.subprocess.Popen") as launch:
            with CurlTransport(URL, 100, HOST) as transport:
                for start, end, etag in bad:
                    with self.subTest(start=start, end=end, quoted=etag.startswith('"')):
                        with self.assertRaises(TransferError) as caught:
                            transport.fetch(start, end, etag)
                        self.assert_safe_error(caught.exception)
                self.assertEqual(transport.attempted_requests, 0)
            launch.assert_not_called()

    def test_early_generator_abandonment_is_cleaned_by_context(self):
        process = FakeProcess(wire(200, b"x" * (BLOCK + 10)))
        with patch("curl_transport.subprocess.Popen", return_value=process):
            with CurlTransport(URL, BLOCK + 10, HOST) as transport:
                chunks, status, headers = transport.origin()
                self.assertEqual(len(next(chunks)), BLOCK)
                self.assertEqual(len(transport._owned), 1)
            self.assertEqual(process.terminated, 1)
            self.assertTrue(process.stdout.closed)
            self.assertEqual(len(transport._owned), 0)
            with self.assertRaises(TransferError):
                status()
            chunks.close()

    def test_second_origin_or_concurrent_curl_is_refused(self):
        process = FakeProcess(wire(200, b"x" * (BLOCK + 1)))
        with patch("curl_transport.subprocess.Popen", return_value=process) as launch:
            with CurlTransport(URL, BLOCK + 1, HOST) as transport:
                chunks, status, headers = transport.origin()
                with self.assertRaises(TransferError):
                    transport.origin()
                next(chunks)
                with self.assertRaises(TransferError):
                    transport.fetch(0, 3, ETAG)
                self.assertEqual(launch.call_count, 1)
                chunks.close()
            self.assertTrue(process.stdout.closed)

    def test_malformed_folded_oversized_and_interim_headers_fail_closed(self):
        bad = [
            b"HTTP/2 200 Fixture\r\n\r\nx",
            b"HTTP/1.1 200 Fixture\n\nx",
            b"HTTP/1.1 200 Fixture\r\n Folded: private\r\n\r\nx",
            b"HTTP/1.1 200 Fixture\r\nBad Name: private\r\n\r\nx",
            b"HTTP/1.1 200 Fixture\r\nX: " + b"x" * 8193 + b"\r\n\r\nx",
            b"HTTP/1.1 100 Continue\r\n\r\n" * 6 + wire(200, b"x"),
            b"HTTP/1.1 101 Switching\r\n\r\nx"]
        for i, content in enumerate(bad):
            with self.subTest(case=i):
                process = FakeProcess(content)
                with patch("curl_transport.subprocess.Popen", return_value=process):
                    with CurlTransport(URL, 1, HOST) as transport:
                        chunks, _, _ = transport.origin()
                        with self.assertRaises(TransferError) as caught:
                            list(chunks)
                        self.assert_safe_error(caught.exception)
                    self.assertTrue(process.stdout.closed)

    def test_redirect_headers_are_not_followed_or_published(self):
        content = wire(302, b"", [("Location", URL), ("Content-Length", "0")])
        process = FakeProcess(content)
        with patch("curl_transport.subprocess.Popen", return_value=process) as launch:
            with CurlTransport(URL, 1, HOST) as transport:
                chunks, _, _ = transport.origin()
                with self.assertRaises(TransferError) as caught:
                    list(chunks)
                self.assert_safe_error(caught.exception)
                self.assertEqual(launch.call_count, 1)
                self.assertNotIn("--location", launch.call_args.args[0])
                self.assertEqual(transport.received_body_bytes, 0)

    def test_process_spawn_and_nonzero_errors_do_not_expose_URL(self):
        with patch("curl_transport.subprocess.Popen", side_effect=OSError(URL)):
            with CurlTransport(URL, 1, HOST) as transport:
                chunks, _, _ = transport.origin()
                with self.assertRaises(TransferError) as caught:
                    list(chunks)
                self.assert_safe_error(caught.exception)
        process = FakeProcess(wire(200, b"x"), code=22)
        with patch("curl_transport.subprocess.Popen", return_value=process):
            with CurlTransport(URL, 1, HOST) as transport:
                chunks, _, _ = transport.origin()
                with self.assertRaises(TransferError) as caught:
                    list(chunks)
                self.assert_safe_error(caught.exception)
                self.assertEqual(transport.completed_requests, 0)

    def test_timeout_is_sanitized_and_owned_kill_is_bounded(self):
        secret_timeout = lambda: subprocess.TimeoutExpired(["curl", URL], 5)
        process = FakeProcess(wire(200, b"x"),
                              waits=[secret_timeout(), secret_timeout(), -9])
        with patch("curl_transport.subprocess.Popen", return_value=process):
            with CurlTransport(URL, 1, HOST) as transport:
                chunks, _, _ = transport.origin()
                with self.assertRaises(TransferError) as caught:
                    list(chunks)
                self.assert_safe_error(caught.exception)
                self.assertEqual(process.terminated, 1)
                self.assertEqual(process.killed, 1)
                self.assertEqual(len(transport._owned), 0)
            self.assertTrue(process.stdout.closed)

if __name__ == "__main__":
    unittest.main(verbosity=2)
