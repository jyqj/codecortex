"""Complete stdlib ZIP CRC/SHA intake, without extraction or whole nested buffers."""
from collections import Counter
import hashlib
import io
import json
from pathlib import PurePosixPath
import stat
import zipfile

from verified_range import MAX_READ, require

MAX_MEMBERS = 25000  # original outer 10,738 plus at most its own original-member population
MAX_EXPANDED = 4 * 1024**3
MAX_METADATA_JSON = 8 * 1024**2
MAX_SELECTED_JSON = 16 * 1024**2
MAX_SELECTED_FILE = 4 * 1024**2
STREAM_CHUNK = 64 * 1024

class IntakeBudget:
    def __init__(self):
        self.member_count = 0
        self.metadata_bytes = 0
        self.selected_bytes = 0

    def member(self, name, row):
        self.member_count += 1
        require(self.member_count <= MAX_MEMBERS, "combined member population budget")
        self.metadata_bytes += len(json.dumps([name, row], ensure_ascii=True).encode())
        require(self.metadata_bytes <= MAX_METADATA_JSON, "combined member metadata budget")

    def selected(self, declared_size):
        require(0 <= declared_size <= MAX_SELECTED_FILE, "selected file size budget")
        require(self.selected_bytes + declared_size <= MAX_SELECTED_JSON,
                "combined selected bytes budget")
        self.selected_bytes += declared_size

class CountedSeek:
    """128 KiB tail window; all real forward reads are at most 64 KiB.

    Logical back-seeks within the retained tail do not re-inflate the stream.
    Earlier targets reset once, then replay through bounded reads. Counters are
    real API bytes/events and logical distances, not inferred inflate CPU work.
    """
    TAIL_BYTES = 128 * 1024

    def __init__(self, stream, size):
        require(type(size) is int and size >= 0 and stream.tell() == 0, "nested registered size/origin")
        self.stream = stream
        self.size = size
        self.position = 0
        self.raw_position = 0
        self.tail = bytearray()
        self.read_calls = 0
        self.returned_bytes = 0
        self.underlying_read_calls = 0
        self.underlying_read_bytes = 0
        self.seek_calls = 0
        self.backward_seeks = 0
        self.forward_seek_distance = 0
        self.backward_seek_distance = 0
        self.seek_discarded_bytes = 0
        self.reset_calls = 0
        self.tail_seek_hits = 0
        self.maximum_tail_bytes = 0

    def read(self, size=-1):
        require(type(size) is int, "noninteger nested read")
        if size < 0:
            size = max(0, self.size - self.position)
        require(size <= MAX_READ, "unbounded nested read")
        remaining = min(size, self.size - self.position)
        pieces = []
        if remaining and self.position < self.raw_position:
            start = self.raw_position - len(self.tail)
            require(self.position >= start, "nested cursor outside retained tail")
            amount = min(remaining, self.raw_position - self.position)
            offset = self.position - start
            pieces.append(bytes(self.tail[offset:offset + amount]))
            self.position += amount
            remaining -= amount
        while remaining:
            require(self.position == self.raw_position, "nested forward cursor mismatch")
            amount = min(STREAM_CHUNK, remaining)
            block = self.stream.read(amount)
            require(0 < len(block) <= amount, "nested bounded read progress")
            self.underlying_read_calls += 1
            self.underlying_read_bytes += len(block)
            self.raw_position += len(block)
            self.position += len(block)
            remaining -= len(block)
            self.tail.extend(block)
            if len(self.tail) > self.TAIL_BYTES:
                del self.tail[:-self.TAIL_BYTES]
            self.maximum_tail_bytes = max(self.maximum_tail_bytes, len(self.tail))
            pieces.append(block)
        result = b"".join(pieces)
        self.read_calls += 1
        self.returned_bytes += len(result)
        return result

    def tell(self):
        return self.position

    def seek(self, offset, whence=0):
        require(type(offset) is int, "noninteger nested seek")
        old = self.position
        if whence == io.SEEK_SET:
            target = offset
        elif whence == io.SEEK_CUR:
            target = old + offset
        elif whence == io.SEEK_END:
            target = self.size + offset
        else:
            raise ValueError("unknown nested seek whence")
        require(0 <= target <= self.size, "nested seek outside registered bytes")
        self.seek_calls += 1
        if target < old:
            self.backward_seeks += 1
            self.backward_seek_distance += old - target
        else:
            self.forward_seek_distance += target - old
        if self.raw_position - len(self.tail) <= target <= self.raw_position:
            self.position = target
            self.tail_seek_hits += 1
            return target
        if target < self.raw_position - len(self.tail):
            require(self.stream.seek(0) == 0, "nested reset differs")
            self.position = self.raw_position = 0
            self.tail.clear()
            self.reset_calls += 1
        else:
            # Jump over bytes already materialized in the bounded tail.
            self.position = self.raw_position
        while self.position < target:
            amount = min(STREAM_CHUNK, target - self.position)
            before = self.underlying_read_bytes
            self.read(amount)
            self.seek_discarded_bytes += self.underlying_read_bytes - before
        require(self.position == target, "nested bounded seek position")
        return target

    def seekable(self):
        return True

    def counters(self):
        return dict(read_calls=self.read_calls, returned_bytes=self.returned_bytes,
                    underlying_read_calls=self.underlying_read_calls,
                    underlying_read_bytes=self.underlying_read_bytes,
                    seek_calls=self.seek_calls, backward_seeks=self.backward_seeks,
                    forward_seek_distance=self.forward_seek_distance,
                    backward_seek_distance=self.backward_seek_distance,
                    seek_discarded_bytes=self.seek_discarded_bytes, reset_calls=self.reset_calls,
                    tail_seek_hits=self.tail_seek_hits, maximum_tail_bytes=self.maximum_tail_bytes,
                    scope="actual stream API calls/bytes and logical distances; no inferred inflate CPU count")

def safe_infos(zipped):
    infos = zipped.infolist()
    require(len(infos) <= MAX_MEMBERS and len({x.filename for x in infos}) == len(infos),
            "duplicate or excessive ZIP member population")
    require(sum(x.file_size for x in infos) <= MAX_EXPANDED, "ZIP expanded byte budget")
    for info in infos:
        name = info.filename
        path = PurePosixPath(name)
        require(not info.is_dir() and not path.is_absolute() and ".." not in path.parts
                and path.as_posix() == name and "\\" not in name and "\0" not in name
                and info.orig_filename == name and len(name.encode()) <= 1024,
                "unsafe ZIP path")
        require(stat.S_IFMT(info.external_attr >> 16) in (0, stat.S_IFREG)
                and not info.flag_bits & 1, "encrypted or nonregular ZIP member")
        require(info.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                "unsupported ZIP compression method")
        require(info.volume == 0, "multidisk ZIP member")
        require(info.file_size >= 0 and info.compress_size >= 0 and info.header_offset >= 0,
                "invalid ZIP size/offset")
    # Header order reduces seeks. No claim of a single pass through nested deflate.
    return sorted(infos, key=lambda x: x.header_offset)

def audit_zip(zipped, budget, select=()):
    selected_names = set(select)
    infos = safe_infos(zipped)
    require(selected_names <= {x.filename for x in infos}, "missing selected ZIP members")
    members = {}
    selected = {}
    for info in infos:
        capture = info.filename in selected_names
        if capture:
            budget.selected(info.file_size)
        chunks = []
        h = hashlib.sha256()
        size = 0
        with zipped.open(info) as stream:
            while True:
                block = stream.read(STREAM_CHUNK)
                if not block:
                    break
                size += len(block)
                require(size <= info.file_size, "expanded ZIP exceeds central size")
                h.update(block)
                if capture:
                    chunks.append(block)
        # zipfile checks its original stored CRC on complete consumption.
        require(size == info.file_size, "expanded ZIP central size differs")
        row = dict(bytes=size, sha256=h.hexdigest(), crc32="%08x" % info.CRC)
        budget.member(info.filename, row)
        members[info.filename] = row
        if capture:
            selected[info.filename] = b"".join(chunks)
    return members, selected

def compare_expanded(inner_members, outer_members, prefix):
    actual_names = {n for n in outer_members if n.startswith(prefix)}
    require(actual_names == {prefix + n for n in inner_members}, "nested/extracted population differs")
    for name, row in inner_members.items():
        outer = outer_members[prefix + name]
        require((row["bytes"], row["sha256"]) == (outer["bytes"], outer["sha256"]),
                "nested/extracted byte identity differs")

def check_seals(members, selected):
    for prefix in ("p8-build/", "p8-runtime/"):
        name = prefix + "seal.json"
        require(name in selected, "mandatory selected seal absent")
        seal = json.loads(selected[name])["artifact_inventory"]
        actual = {n[len(prefix):]: {"bytes": row["bytes"], "sha256": row["sha256"]}
                  for n, row in members.items() if n.startswith(prefix) and n != name}
        require(seal == actual, "complete original seal inventory differs")
