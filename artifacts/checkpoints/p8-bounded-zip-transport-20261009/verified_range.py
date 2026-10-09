"""Bounded, read-only transport for a previously registered ZIP.

No product or statistics execution. ZIP parsing/CRC semantics are Python zipfile's,
not a new local-header/descriptor validator. The complete registered ZIP digest
and initial block hashes bind every later byte consumed by zipfile.
"""
from collections import OrderedDict
from dataclasses import dataclass
import hashlib
import io
import time

BLOCK = 256 * 1024
CACHE_BLOCKS = 8
MAX_RANGE_REQUESTS = 4096
TRANSPORT_MULTIPLIER = 4  # includes the initial complete SHA scan
MAX_READ = 4 * 1024 * 1024
WORKING_BYTES = 96 * 1024 * 1024
RESERVE_BYTES = 512 * 1024 * 1024

class TransportError(ValueError):
    pass

def require(ok, message):
    if not ok:
        raise TransportError(message)

def normalized_headers(pairs):
    result = {}
    for name, value in pairs:
        key = name.strip().lower()
        require(key and key not in result, "duplicate or empty response header")
        require("\r" not in value and "\n" not in value, "multiline response header")
        result[key] = value.strip()
    return result

@dataclass(frozen=True)
class Origin:
    size: int
    sha256: str
    etag: str
    block_sha256: tuple
    block_size: int = BLOCK

def scan_origin(chunks, status, header_pairs, size, expected_sha256):
    """Consume every original byte once, retaining only fixed-block digests."""
    require(type(size) is int and size > 0, "invalid registered size")
    require(len(expected_sha256) == 64 and all(c in "0123456789abcdef" for c in expected_sha256),
            "invalid registered digest")
    whole = hashlib.sha256()
    part = hashlib.sha256()
    in_part = 0
    total = 0
    hashes = []
    for chunk in chunks:
        require(isinstance(chunk, bytes) and 0 < len(chunk) <= BLOCK, "unbounded initial chunk")
        total += len(chunk)
        require(total <= size, "initial body exceeds registration")
        whole.update(chunk)
        position = 0
        while position < len(chunk):
            length = min(BLOCK - in_part, len(chunk) - position)
            part.update(memoryview(chunk)[position:position + length])
            in_part += length
            position += length
            if in_part == BLOCK:
                hashes.append(part.hexdigest())
                part = hashlib.sha256()
                in_part = 0
    if in_part:
        hashes.append(part.hexdigest())
    headers = normalized_headers(header_pairs() if callable(header_pairs) else header_pairs)
    require((status() if callable(status) else status) == 200, "initial response must be HTTP 200")
    require(headers.get("content-length") == str(size), "initial content length")
    require(headers.get("content-encoding", "identity").lower() == "identity", "encoded initial body")
    etag = headers.get("etag", "")
    require(bool(etag) and len(etag) <= 512, "missing or oversized initial ETag")
    require(total == size and whole.hexdigest() == expected_sha256, "initial complete ZIP identity")
    require(len(hashes) == (size + BLOCK - 1) // BLOCK, "initial block population")
    return Origin(size, expected_sha256, etag, tuple(hashes))

class VerifiedRangeReader(io.RawIOBase):
    """fetch(start,end,etag) returns (status, header_pairs, exact body bytes).

    A fetch is admitted only for one complete initially-hashed block. The last
    block is shorter. Every request and received body is counted, including a
    rejected response. No retries or budget enlargement are performed here.
    """
    def __init__(self, origin, fetch, monotonic=time.monotonic, deadline_seconds=1800):
        super().__init__()
        require(isinstance(origin, Origin), "unverified origin")
        require(origin.block_size == BLOCK, "unsupported block size")
        require(len(origin.block_sha256) == (origin.size + BLOCK - 1) // BLOCK,
                "origin block population")
        require(type(deadline_seconds) in (int, float) and 0 < deadline_seconds <= 1800,
                "transport deadline")
        self.origin = origin
        self.fetch = fetch
        self.clock = monotonic
        self.deadline = monotonic() + deadline_seconds
        self.position = 0
        self.cache = OrderedDict()
        self.range_requests = 0
        self.range_bytes = 0
        self.initial_bytes = origin.size
        self.network_bytes = origin.size
        self.cache_hits = 0
        self.bytes_returned = 0
        self.seek_count = 0
        self.backward_seek_count = 0
        self.maximum_cache_bytes = 0

    def readable(self):
        return True

    def seekable(self):
        return True

    def tell(self):
        self._checkClosed()
        return self.position

    def seek(self, offset, whence=io.SEEK_SET):
        self._checkClosed()
        require(type(offset) is int, "noninteger seek")
        if whence == io.SEEK_SET:
            position = offset
        elif whence == io.SEEK_CUR:
            position = self.position + offset
        elif whence == io.SEEK_END:
            position = self.origin.size + offset
        else:
            raise TransportError("unknown seek whence")
        require(position >= 0, "negative seek")
        self.seek_count += 1
        self.backward_seek_count += int(position < self.position)
        self.position = position
        return position

    def _block(self, index):
        if index in self.cache:
            self.cache_hits += 1
            self.cache.move_to_end(index)
            return self.cache[index]
        require(self.clock() <= self.deadline, "transport deadline exceeded")
        require(self.range_requests < MAX_RANGE_REQUESTS, "transport request budget")
        start = index * BLOCK
        end = min(start + BLOCK, self.origin.size) - 1
        require(0 <= start <= end < self.origin.size, "block outside original")
        expected_bytes = end - start + 1
        require(self.network_bytes + expected_bytes <= TRANSPORT_MULTIPLIER * self.origin.size,
                "transport total-byte budget")
        self.range_requests += 1
        status, pairs, body = self.fetch(start, end, self.origin.etag)
        require(isinstance(body, bytes), "nonbyte range body")
        self.range_bytes += len(body)
        self.network_bytes += len(body)
        require(self.network_bytes <= TRANSPORT_MULTIPLIER * self.origin.size,
                "received transport total-byte budget")
        headers = normalized_headers(pairs)
        require(status == 206, "range response must be HTTP 206")
        require(headers.get("content-range") == "bytes %d-%d/%d" % (start, end, self.origin.size),
                "range Content-Range differs")
        require(headers.get("content-length") == str(expected_bytes), "range Content-Length differs")
        require(headers.get("content-encoding", "identity").lower() == "identity", "encoded range body")
        require(headers.get("etag") == self.origin.etag, "range ETag differs")
        require(len(body) == expected_bytes, "range actual body size differs")
        require(hashlib.sha256(body).hexdigest() == self.origin.block_sha256[index],
                "range bytes differ from initial complete ZIP")
        self.cache[index] = body
        while len(self.cache) > CACHE_BLOCKS:
            self.cache.popitem(last=False)
        self.maximum_cache_bytes = max(self.maximum_cache_bytes, sum(map(len, self.cache.values())))
        require(self.maximum_cache_bytes <= CACHE_BLOCKS * BLOCK, "LRU memory bound")
        return body

    def read(self, size=-1):
        self._checkClosed()
        require(type(size) is int, "noninteger read")
        if size < 0:
            size = max(0, self.origin.size - self.position)
        require(size <= MAX_READ, "unbounded random-access read")
        remaining = min(size, max(0, self.origin.size - self.position))
        pieces = []
        while remaining:
            index, offset = divmod(self.position, BLOCK)
            block = self._block(index)
            take = min(remaining, len(block) - offset)
            require(take > 0, "range progress")
            pieces.append(block[offset:offset + take])
            self.position += take
            remaining -= take
        result = b"".join(pieces)
        self.bytes_returned += len(result)
        return result

    def readinto(self, buffer):
        data = self.read(len(buffer))
        buffer[:len(data)] = data
        return len(data)

    def counters(self):
        return dict(initial_bytes=self.initial_bytes, range_requests=self.range_requests,
                    range_bytes=self.range_bytes, network_bytes=self.network_bytes,
                    network_limit=TRANSPORT_MULTIPLIER * self.origin.size,
                    range_request_limit=MAX_RANGE_REQUESTS, cache_hits=self.cache_hits,
                    maximum_cache_bytes=self.maximum_cache_bytes,
                    bytes_returned=self.bytes_returned, seek_count=self.seek_count,
                    backward_seek_count=self.backward_seek_count)

    def close(self):
        self.cache.clear()
        super().close()
