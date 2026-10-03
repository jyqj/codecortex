"""Check the pinned public tarball against the complete inclusion inventory.

Does not write source or unpack arbitrary archive paths. No HEAD tracking.
"""
import io
import json
import tarfile
import urllib.request
from author import ROOT, SHA, digest

inventory = json.loads((ROOT / "provenance/inventory.json").read_text())
raw = urllib.request.urlopen(inventory["archive_url"], timeout=60).read()
assert digest(raw) == inventory["archive_sha256"]
expected = {r["path"]:r for r in inventory["files"]}
observed = set()
with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
    for member in archive.getmembers():
        if not member.isfile():
            continue
        path = member.name.split("/", 1)[1]
        assert path in expected
        observed.add(path)
        data = archive.extractfile(member).read()
        entry = expected[path]
        assert digest(data) == entry["sha256"] and len(data) == entry["bytes"]
        if entry["included"]:
            folder = "license" if path in ("LICENSE", "NOTICE") else "source"
            assert (ROOT / folder / path).read_bytes() == data
assert observed == set(expected)
print("Pinned public archive, all inventory hashes, admitted bytes and license match:", SHA)
