"""Complete standard-library ZIP CRC/SHA intake; disk-backed outer/nested files.
This transport is separate from the bounded HTTP Range prototype.
"""
from collections import Counter
import hashlib
import io
import json
from pathlib import PurePosixPath
import stat
import zipfile

MAX_READ = 4 * 1024 * 1024

def require(ok, message):
    if not ok:
        raise ValueError(message)

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
