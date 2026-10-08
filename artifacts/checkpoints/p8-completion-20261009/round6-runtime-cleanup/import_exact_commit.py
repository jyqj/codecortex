"""Import a GitHub commit object only after its exact Git object ID matches.

Read a GitHub Git-commit JSON object from stdin. This writes an object, never a
ref, index, or working-tree file. Candidate date offsets account for GitHub's
UTC-normalized metadata without accepting a different commit representation.
"""
from datetime import datetime
import hashlib
import itertools
import json
import re
import subprocess
import sys


def main():
    record = json.load(sys.stdin)
    expected = record["sha"]
    if not re.fullmatch(r"[0-9a-f]{40}", expected):
        raise ValueError("immutable commit SHA required")

    candidates = []
    verification = record.get("verification") or {}
    payload, signature = verification.get("payload"), verification.get("signature")
    if payload and signature:
        header, separator, body = payload.partition("\n\n")
        if not separator:
            raise ValueError("invalid signed payload")
        for trailing in (0, 1, 2):
            value = signature.rstrip("\n") + "\n" * trailing
            signed = header + "\ngpgsig " + value.replace("\n", "\n ") + "\n\n" + body
            candidates.append(signed.encode())
    else:
        def person(kind, offset):
            value = record[kind]
            timestamp = int(datetime.fromisoformat(value["date"].replace("Z", "+00:00")).timestamp())
            return f"{kind} {value['name']} <{value['email']}> {timestamp} {offset}"

        headers = ["tree " + record["tree"]["sha"]]
        headers.extend("parent " + value["sha"] for value in record["parents"])
        for author_offset, committer_offset, trailing in itertools.product(
                ("+0000", "+0800", "-0700", "-0800"),
                ("+0000", "+0800", "-0700", "-0800"), (0, 1, 2)):
            header = "\n".join(headers + [person("author", author_offset), person("committer", committer_offset)])
            candidates.append((header + "\n\n" + record["message"].rstrip("\n") + "\n" * trailing).encode())

    for raw in candidates:
        digest = hashlib.sha1(b"commit " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        if digest == expected:
            actual = subprocess.check_output(["git", "hash-object", "-t", "commit", "-w", "--stdin"], input=raw).decode().strip()
            if actual != expected:
                raise ValueError("written Git object differs")
            print(actual)
            return
    raise ValueError("no exact Git commit representation matched; no object imported")


if __name__ == "__main__":
    main()
