"""Strict libtest population validation; never substitutes for test execution."""
import re


def validate_test_stdout(text, expected):
    if not expected or len(expected) != len(set(expected)):
        raise ValueError("expected test population must be nonempty and unique")
    running = re.findall(r"^running (\d+) tests?\s*$", text, re.M)
    if running != [str(len(expected))]:
        raise ValueError("missing, repeated, or wrong running test count")
    lines = [line for line in text.splitlines()
             if line.startswith("test ") and not line.startswith("test result:")]
    rows = []
    for line in lines:
        match = re.fullmatch(r"test (.+) \.\.\. (\S+)", line)
        if not match or match.group(2) != "ok":
            raise ValueError("malformed, failed, or ignored test line")
        rows.append(match.group(1))
    if sorted(rows) != sorted(expected):
        raise ValueError("actual test names differ from the exact registered population")
    summaries = [line for line in text.splitlines() if line.startswith("test result:")]
    if len(summaries) != 1:
        raise ValueError("missing or repeated libtest summary")
    match = re.fullmatch(
        r"test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; "
        r"\d+ filtered out; finished in [0-9.]+s", summaries[0])
    if not match or int(match.group(1)) != len(expected):
        raise ValueError("failed, ignored, measured, or inconsistent libtest summary")
    return {"expected_count": len(expected), "actual_names": sorted(rows),
            "passed": len(rows), "failed": 0, "ignored": 0, "measured": 0}
