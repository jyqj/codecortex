#!/usr/bin/env python3
"""Independent SQLite mechanism control, not a codecortex product execution."""
import json
import os
from pathlib import Path
import sqlite3
import tempfile


def reproduce(retire_writer):
    with tempfile.TemporaryDirectory(prefix="p7-review-sqlite-") as directory:
        root = Path(directory)
        live, staging, parked = root / "index.db", root / "staging.db", root / "published.db"
        writer = sqlite3.connect(live)
        writer.executescript("PRAGMA journal_mode=WAL; CREATE TABLE sentinel(k TEXT PRIMARY KEY);"
                             "INSERT INTO sentinel VALUES('old');")
        reader = sqlite3.connect(live)
        assert reader.execute("SELECT k FROM sentinel").fetchall() == [("old",)]
        fresh = sqlite3.connect(staging)
        fresh.executescript("CREATE TABLE sentinel(k TEXT PRIMARY KEY); INSERT INTO sentinel VALUES('new');")
        fresh.close()
        checkpoint = writer.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
        assert checkpoint == (0, 0, 0)
        if retire_writer:
            writer.execute("PRAGMA query_only=ON")
        for suffix in ("-wal", "-shm"):
            Path(str(live) + suffix).unlink(missing_ok=True)
        os.replace(staging, live)

        # Reproduce the real reopen failure with an owned directory at the
        # authoritative path. Restore the successfully published file before
        # testing the old connection's behavior after the error was returned.
        os.replace(live, parked)
        live.mkdir()
        try:
            should_fail = sqlite3.connect(live)
        except sqlite3.OperationalError as error:
            reopen_error = str(error)
        else:
            should_fail.close()
            raise AssertionError("opening a directory unexpectedly succeeded")
        live.rmdir()
        os.replace(parked, live)
        try:
            writer.execute("INSERT INTO sentinel VALUES('after-returned-error')")
            writer.commit()
        except sqlite3.OperationalError as error:
            write_outcome = {"accepted": False, "error": str(error)}
        else:
            write_outcome = {"accepted": True}
        authoritative = sqlite3.connect(live)
        new_rows = authoritative.execute("SELECT k FROM sentinel ORDER BY k").fetchall()
        old_rows = reader.execute("SELECT k FROM sentinel ORDER BY k").fetchall()
        assert new_rows == [("new",)]
        assert write_outcome["accepted"] is not retire_writer
        authoritative.close()
        reader.close()
        writer.close()
        return {"retire_old_writer_query_only": retire_writer, "checkpoint": checkpoint,
                "reopen_error": reopen_error, "old_writer_after_error": write_outcome,
                "authoritative_rows": new_rows, "old_reader_rows": old_rows}


if __name__ == "__main__":
    print(json.dumps({"scope": "independent SQLite mechanism; actual product tests are separately bound",
                      "sqlite_version": sqlite3.sqlite_version,
                      "cases": [reproduce(False), reproduce(True)]}, indent=2))
