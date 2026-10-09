import hashlib
import io
import json
import stat
import struct
import unittest
import warnings
import zipfile
from unittest.mock import patch

import verified_range as vr
import zip_intake as zi

ETAG = '"fixed-test-origin"'

def headers(raw):
    return [("Content-Length", str(len(raw))), ("ETag", ETAG)]

def origin(raw):
    return vr.scan_origin((raw[n:n + 65536] for n in range(0, len(raw), 65536)),
                          200, headers(raw), len(raw), hashlib.sha256(raw).hexdigest())

class FakeFetch:
    def __init__(self, raw, change=None):
        self.raw = raw
        self.change = change
        self.calls = []

    def __call__(self, start, end, etag):
        self.calls.append((start, end, etag))
        body = self.raw[start:end + 1]
        pairs = [("Content-Range", "bytes %d-%d/%d" % (start, end, len(self.raw))),
                 ("Content-Length", str(len(body))), ("ETag", ETAG)]
        result = (206, pairs, body)
        return self.change(*result) if self.change else result

def make_zip(files, method=zipfile.ZIP_DEFLATED):
    out = io.BytesIO()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        with zipfile.ZipFile(out, "w", compression=method) as z:
            for name, body in files:
                z.writestr(name, body)
    return out.getvalue()

def audit(raw, selected=()):
    fetch = FakeFetch(raw)
    reader = vr.VerifiedRangeReader(origin(raw), fetch)
    with zipfile.ZipFile(reader) as z:
        result = zi.audit_zip(z, zi.IntakeBudget(), selected)
    return result, reader.counters()

class RangeControls(unittest.TestCase):
    def setUp(self):
        self.raw = (bytes(range(256)) * (vr.BLOCK * 10 // 256)) + b"tail!"
        self.origin = origin(self.raw)

    def rejected_fetch(self, change):
        reader = vr.VerifiedRangeReader(self.origin, FakeFetch(self.raw, change))
        with self.assertRaises(vr.TransportError):
            reader.read(1)

    def test_initial_full_digest_length_and_status(self):
        sha = hashlib.sha256(self.raw).hexdigest()
        for status, pairs, size, digest in (
            (206, headers(self.raw), len(self.raw), sha),
            (200, headers(self.raw), len(self.raw), "0" * 64),
            (200, headers(self.raw), len(self.raw) + 1, sha),
            (200, [("Content-Length", str(len(self.raw)))], len(self.raw), sha),
        ):
            with self.subTest(status=status, size=size, digest=digest[:2], pairs=pairs):
                with self.assertRaises(vr.TransportError):
                    vr.scan_origin((self.raw[n:n+vr.BLOCK] for n in range(0,len(self.raw),vr.BLOCK)),
                                   status, pairs, size, digest)

    def test_deferred_initial_headers(self):
        result = vr.scan_origin((b"abc",), lambda: 200, lambda: headers(b"abc"), 3,
                                hashlib.sha256(b"abc").hexdigest())
        self.assertEqual(result.block_sha256, (hashlib.sha256(b"abc").hexdigest(),))

    def test_cross_block_tail_and_bounded_read_all(self):
        reader = vr.VerifiedRangeReader(self.origin, FakeFetch(self.raw))
        reader.seek(vr.BLOCK - 7)
        self.assertEqual(reader.read(22), self.raw[vr.BLOCK - 7:vr.BLOCK + 15])
        reader.seek(-5, 2)
        self.assertEqual(reader.read(), b"tail!")
        reader.seek(len(self.raw) + 100)
        self.assertEqual(reader.read(1), b"")
        reader.seek(0)
        self.assertEqual(reader.read(), self.raw)
        bigger = origin(b"x" * (vr.MAX_READ + 1))
        with self.assertRaises(vr.TransportError):
            vr.VerifiedRangeReader(bigger, FakeFetch(b"x" * bigger.size)).read()

    def test_lru_eviction_revalidates_and_stays_bounded(self):
        fetch = FakeFetch(self.raw)
        reader = vr.VerifiedRangeReader(self.origin, fetch)
        for n in range(10):
            reader.seek(n * vr.BLOCK)
            self.assertEqual(reader.read(1), self.raw[n * vr.BLOCK:n * vr.BLOCK + 1])
        reader.seek(0)
        reader.read(1)
        self.assertEqual(len(fetch.calls), 11)
        self.assertEqual(reader.maximum_cache_bytes, vr.BLOCK * vr.CACHE_BLOCKS)

    def test_200_is_not_a_range(self):
        self.rejected_fetch(lambda status, pairs, body: (200, pairs, body))

    def test_each_content_range_coordinate_is_exact(self):
        for value in ("bytes 1-262143/2621445", "bytes 0-262142/2621445",
                      "bytes 0-262143/2621446"):
            with self.subTest(value=value):
                self.rejected_fetch(lambda status, pairs, body:
                                    (status, [("Content-Range", value)] + pairs[1:], body))

    def test_short_and_long_range_bodies(self):
        for delta in (-1, 1):
            with self.subTest(delta=delta):
                self.rejected_fetch(lambda status, pairs, body:
                                    (status, pairs, body[:-1] if delta < 0 else body + b"x"))

    def test_same_etag_changed_bytes_rejected_by_initial_block_sha(self):
        self.rejected_fetch(lambda status, pairs, body: (status, pairs, b"x" + body[1:]))

    def test_changed_etag_rejected(self):
        self.rejected_fetch(lambda status, pairs, body:
                            (status, pairs[:-1] + [("ETag", '"changed"')], body))

    def test_duplicate_header_content_length_and_encoding_rejected(self):
        changes = [
            lambda status, pairs, body: (status, pairs + [("ETag", ETAG)], body),
            lambda status, pairs, body: (status, [pairs[0], ("Content-Length", "1"), pairs[2]], body),
            lambda status, pairs, body: (status, pairs + [("Content-Encoding", "gzip")], body),
        ]
        for change in changes:
            with self.subTest(change=change):
                self.rejected_fetch(change)

    def test_request_total_bytes_and_deadline_budgets(self):
        reader = vr.VerifiedRangeReader(self.origin, FakeFetch(self.raw))
        reader.range_requests = vr.MAX_RANGE_REQUESTS
        with self.assertRaises(vr.TransportError):
            reader.read(1)
        reader = vr.VerifiedRangeReader(self.origin, FakeFetch(self.raw))
        reader.network_bytes = vr.TRANSPORT_MULTIPLIER * self.origin.size
        with self.assertRaises(vr.TransportError):
            reader.read(1)
        clock = [0]
        reader = vr.VerifiedRangeReader(self.origin, FakeFetch(self.raw), monotonic=lambda: clock[0])
        clock[0] = 1801
        with self.assertRaises(vr.TransportError):
            reader.read(1)

    def test_negative_seek_and_oversized_read_rejected(self):
        reader = vr.VerifiedRangeReader(self.origin, FakeFetch(self.raw))
        for action in (lambda: reader.seek(-1), lambda: reader.seek(0, 3),
                       lambda: reader.read(vr.MAX_READ + 1)):
            with self.subTest(action=action):
                with self.assertRaises(vr.TransportError):
                    action()

class ZipControls(unittest.TestCase):
    def test_stored_and_deflated_all_bytes_match_stdlib(self):
        for method in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
            with self.subTest(method=method):
                raw = make_zip([("a.json", b'{"a":1}'), ("b.bin", bytes(range(256)) * 8)], method)
                (members, selected), counters = audit(raw, {"a.json"})
                self.assertEqual(selected["a.json"], b'{"a":1}')
                with zipfile.ZipFile(io.BytesIO(raw)) as expected:
                    for name, row in members.items():
                        body = expected.read(name)
                        self.assertEqual(row["sha256"], hashlib.sha256(body).hexdigest())
                        self.assertEqual(row["bytes"], len(body))
                self.assertGreater(counters["range_requests"], 0)

    def test_descriptor_and_forced_local_zip64_via_stdlib(self):
        class NonSeekable(io.BytesIO):
            def seek(self, *args):
                raise OSError("fixture deliberately not seekable")
        for method in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
            with self.subTest(method=method):
                output = NonSeekable()
                with zipfile.ZipFile(output, "w", compression=method) as z:
                    with z.open("forced64", "w", force_zip64=True) as stream:
                        stream.write(b"descriptor-and-zip64" * 100)
                raw = output.getvalue()
                (members, _), _ = audit(raw)
                self.assertEqual(members["forced64"]["bytes"], 2000)
                with zipfile.ZipFile(io.BytesIO(raw)) as z:
                    self.assertTrue(z.getinfo("forced64").flag_bits & 8)

    def test_nested_no_whole_zip_buffer_and_expanded_identity(self):
        inner = make_zip([("inside/a", b"a" * 100), ("inside/b", b"b" * 200)])
        raw = make_zip([("original.zip", inner), ("expanded/inside/a", b"a" * 100),
                        ("expanded/inside/b", b"b" * 200)])
        reader = vr.VerifiedRangeReader(origin(raw), FakeFetch(raw))
        budget = zi.IntakeBudget()
        with zipfile.ZipFile(reader) as outer:
            outer_members, _ = zi.audit_zip(outer, budget)
            with outer.open("original.zip") as original:
                counter = zi.CountedSeek(original, len(inner))
                with zipfile.ZipFile(counter) as nested:
                    nested_members, _ = zi.audit_zip(nested, budget)
                self.assertGreater(counter.counters()["backward_seeks"], 0)
            zi.compare_expanded(nested_members, outer_members, "expanded/")
            changed = dict(outer_members)
            changed["expanded/inside/a"] = dict(changed["expanded/inside/a"], sha256="0" * 64)
            with self.assertRaises(vr.TransportError):
                zi.compare_expanded(nested_members, changed, "expanded/")

    def test_large_nested_stored_and_incompressible_deflate_bound_every_seek_read(self):
        # This crosses the production 4 MiB random-read limit. Bytes are
        # deterministic and incompressible enough to exercise outer range reads.
        import random
        rng = random.Random(12345)
        payload = rng.randbytes(vr.MAX_READ + 3 * zi.STREAM_CHUNK)
        inner = make_zip([("large.bin", payload)], zipfile.ZIP_STORED)
        for method in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
            with self.subTest(method=method):
                raw = make_zip([("original.zip", inner)], method)
                reader = vr.VerifiedRangeReader(origin(raw), FakeFetch(raw))
                budget = zi.IntakeBudget()
                with zipfile.ZipFile(reader) as outer:
                    with outer.open("original.zip") as original:
                        counter = zi.CountedSeek(original, len(inner))
                        with zipfile.ZipFile(counter) as nested:
                            members, _ = zi.audit_zip(nested, budget)
                        self.assertEqual(members["large.bin"]["sha256"],
                                         hashlib.sha256(payload).hexdigest())
                        self.assertGreater(counter.seek_discarded_bytes, vr.MAX_READ)
                        self.assertGreater(counter.reset_calls, 0)
                        self.assertGreater(counter.tail_seek_hits, 0)
                        self.assertLessEqual(counter.maximum_tail_bytes, 128 * 1024)
                self.assertLessEqual(reader.network_bytes, vr.TRANSPORT_MULTIPLIER * len(raw))

    def test_corrupt_crc_rejected_even_when_corrupt_zip_digest_is_registered(self):
        raw = bytearray(make_zip([("data", b"payload")], zipfile.ZIP_STORED))
        with zipfile.ZipFile(io.BytesIO(raw)) as z:
            info = z.getinfo("data")
            offset = info.header_offset + 30 + len(info.filename.encode()) + len(info.extra)
        raw[offset] ^= 1
        with self.assertRaises(zipfile.BadZipFile):
            audit(bytes(raw))

    def test_duplicate_escape_directory_and_nonregular_rejected(self):
        cases = [
            make_zip([("same", b"a"), ("same", b"b")]),
            make_zip([("../escape", b"x")]),
            make_zip([("/absolute", b"x")]),
            make_zip([("back\\slash", b"x")]),
            make_zip([("dir/", b"")]),
        ]
        link = zipfile.ZipInfo("link")
        link.create_system = 3
        link.external_attr = (stat.S_IFLNK | 0o777) << 16
        cases.append(make_zip([(link, b"target")]))
        for raw in cases:
            with self.subTest(size=len(raw)):
                with self.assertRaises(vr.TransportError):
                    audit(raw)

    def test_encryption_and_unknown_compression_rejected(self):
        raw = make_zip([("a", b"x")], zipfile.ZIP_STORED)
        central = raw.index(b"PK\x01\x02")
        for offset, value in ((8, 1), (10, 99)):
            modified = bytearray(raw)
            struct.pack_into("<H", modified, central + offset, value)
            with self.subTest(offset=offset):
                with self.assertRaises(vr.TransportError):
                    audit(bytes(modified))

    def test_selected_file_and_combined_budgets_and_missing_member(self):
        raw = make_zip([("a", b"abcd")])
        with patch.object(zi, "MAX_SELECTED_FILE", 3):
            with self.assertRaises(vr.TransportError):
                audit(raw, {"a"})
        with patch.object(zi, "MAX_SELECTED_JSON", 3):
            with self.assertRaises(vr.TransportError):
                audit(raw, {"a"})
        with patch.object(zi, "MAX_METADATA_JSON", 1):
            with self.assertRaises(vr.TransportError):
                audit(raw)
        with self.assertRaises(vr.TransportError):
            audit(raw, {"missing"})

    def test_original_seal_complete_population_and_mutation(self):
        file = b"original"
        row = dict(bytes=len(file), sha256=hashlib.sha256(file).hexdigest())
        items = []
        for prefix in ("p8-build/", "p8-runtime/"):
            items.extend([(prefix + "data", file),
                          (prefix + "seal.json", json.dumps({"artifact_inventory": {"data": row}}).encode())])
        (members, selected), _ = audit(make_zip(items), {"p8-build/seal.json", "p8-runtime/seal.json"})
        zi.check_seals(members, selected)
        altered = dict(members)
        altered["p8-build/data"] = dict(altered["p8-build/data"], bytes=1)
        with self.assertRaises(vr.TransportError):
            zi.check_seals(altered, selected)

if __name__ == "__main__":
    unittest.main(verbosity=2)
