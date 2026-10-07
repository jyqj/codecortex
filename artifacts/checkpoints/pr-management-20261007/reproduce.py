#!/usr/bin/env python3
"""Recheck the fixed PR adoption evidence from local Git objects (stdlib only)."""
import argparse
import collections
import functools
import hashlib
import json
import os
from pathlib import Path
import subprocess


OLD_MAIN = "ff458bc591b4e7e444af4464d6eef2513cdb335c"
SOURCE = "886f90a542a6174a037c79eebbb4f74848fb1f53"
MERGE = "8d6f38197c5a4431423833d99b7d77781daa965f"
LEDGER_COMMIT = "063adeada4720b0fdab07493494bca0ebdcd47e2"
LEDGER_PATHS = ["docs/roadmap/code-index-v2/05-TODO.md",
                "docs/roadmap/code-index-v2/tasks.json"]
STRICT_CLASSES = {"exact_ancestor", "all_delta_paths_exact", "all_unique_commit_deltas_adopted"}
DIRECTORY = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(value):
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(raw).hexdigest()


class AuditGit:
    def __init__(self, repo):
        self.repo = repo
        self.environment = dict(os.environ, GIT_NO_LAZY_FETCH="1")

    def git(self, *arguments):
        return subprocess.check_output(["git", "--no-replace-objects", *arguments],
                                       cwd=self.repo, env=self.environment)

    def lines(self, *arguments):
        return self.git(*arguments).decode().splitlines()

    def ancestor(self, older, newer):
        result = subprocess.run(["git", "--no-replace-objects", "merge-base", "--is-ancestor",
                                 older, newer], cwd=self.repo, env=self.environment,
                                capture_output=True)
        require(result.returncode in (0, 1), result.stderr.decode())
        return result.returncode == 0

    @functools.lru_cache(maxsize=None)
    def tree(self, commit):
        entries = {}
        for record in self.git("ls-tree", "--full-tree", "-r", "-z", commit).split(b"\0"):
            if record:
                metadata, path = record.split(b"\t", 1)
                # Preserve mode, object type and object hash, including symlinks/submodules.
                entries[path.decode()] = metadata.decode()
        return entries

    def paths(self, row):
        base = self.git("merge-base", row["base_sha"], row["head_sha"]).decode().strip()
        changes = self.git("diff-tree", "--no-commit-id", "--name-status", "--no-renames",
                           "-r", "-z", base, row["head_sha"]).split(b"\0")[:-1]
        require(len(changes) % 2 == 0, "Unexpected name-status output")
        original, accepted = self.tree(row["head_sha"]), self.tree(SOURCE)
        groups = {name: [] for name in ("exact", "evolved", "missing", "deleted_still_absent",
                                       "deleted_reintroduced")}
        for status, raw_path in zip(changes[::2], changes[1::2]):
            path = raw_path.decode()
            if status == b"D":
                group = "deleted_still_absent" if path not in accepted else "deleted_reintroduced"
            elif path not in accepted:
                group = "missing"
            else:
                group = "exact" if original[path] == accepted[path] else "evolved"
            groups[group].append(path)
        return base, {key: sorted(paths) for key, paths in groups.items()}

    @functools.lru_cache(maxsize=None)
    def raw_delta(self, commit):
        return self.git("diff-tree", "--root", "--no-commit-id", "--no-renames", "--raw",
                        "--no-abbrev", "-r", commit)

    @functools.lru_cache(maxsize=None)
    def accepted_deltas(self):
        by_delta = collections.defaultdict(list)
        for commit in self.lines("rev-list", "--no-merges", SOURCE):
            raw = self.raw_delta(commit)
            if raw:
                by_delta[raw].append(commit)
        return by_delta

    def evidence(self, row):
        ancestor = self.ancestor(row["head_sha"], SOURCE)
        evidence = {"head_is_ancestor_of_integrated_source": ancestor}
        if ancestor:
            return evidence, "exact_ancestor"
        base, groups = self.paths(row)
        evidence.update(delta_base=base, path_counts={key: len(value) for key, value in groups.items()},
                        path_groups_sha256=digest(groups))
        # Counts and a full-list digest keep the index compact. --paths emits every path.
        examples = {key: value[:3] for key, value in groups.items()
                    if value and key in {"evolved", "missing", "deleted_reintroduced"}}
        if examples:
            evidence["unresolved_path_examples"] = examples
        paths_equal = not any(groups[key] for key in ("evolved", "missing", "deleted_reintroduced"))
        if paths_equal:
            return evidence, "all_delta_paths_exact"
        unique = self.lines("rev-list", "--no-merges", SOURCE + ".." + row["head_sha"])
        merges = self.lines("rev-list", "--merges", SOURCE + ".." + row["head_sha"])
        matches = {commit: self.accepted_deltas().get(self.raw_delta(commit), []) for commit in unique}
        evidence.update(unique_nonmerge_commits=len(unique),
                        matched_unique_nonmerge_commits=sum(bool(value) for value in matches.values()),
                        unique_merge_commits=merges, original_to_matching_raw_delta_commits=matches)
        if unique and not merges and all(matches.values()):
            return evidence, "all_unique_commit_deltas_adopted"
        category = ("manual_adopted_product_ledger_evolved" if row["number"] == 139
                    else "retain_requires_scope_review")
        return evidence, category

    def ledger_history(self):
        path = LEDGER_PATHS[1]
        parent = self.git("rev-parse", LEDGER_COMMIT + "^").decode().strip()
        before = json.loads(self.git("show", parent + ":" + path))
        after = json.loads(self.git("show", LEDGER_COMMIT + ":" + path))
        integrated = json.loads(self.git("show", SOURCE + ":" + path))
        require({key: value for key, value in before.items() if key != "tasks"} ==
                {key: value for key, value in after.items() if key != "tasks"},
                "PR139 original ledger commit changed root metadata")
        changed = [(old, new) for old, new in zip(before["tasks"], after["tasks"]) if old != new]
        require(len(before["tasks"]) == len(after["tasks"]) and len(changed) == 1,
                "Unexpected PR139 task changes")
        old, new = changed[0]
        field = "implementation_notes"
        require(new["id"] == "P7-014" and
                {key: value for key, value in old.items() if key != field} ==
                {key: value for key, value in new.items() if key != field},
                "PR139 ledger changed more than the P7-014 note")
        require(new[field].startswith(old[field]), "Original ledger change is not append-only")
        appended = new[field][len(old[field]):]
        note = next(task[field] for task in integrated["tasks"] if task["id"] == "P7-014")
        paragraphs = [part for part in note.split("\n\n")
                      if "e3b932ed4b1e197022c0902fd4c11af3e87ae87e" in part]
        require(len(paragraphs) == 1, "Expected one explicit integrated PR139 acceptance note")
        changed_paths = self.lines("diff-tree", "--no-commit-id", "--name-only", "--no-renames",
                                   "-r", LEDGER_COMMIT)
        require(sorted(changed_paths) == LEDGER_PATHS, "Unexpected PR139 ledger paths")
        return {
            "original_commit": LEDGER_COMMIT, "original_parent": parent,
            "original_changed_paths": changed_paths, "task": "P7-014", "field": field,
            "operation": "append_only", "task_statuses_and_root_metadata_changed": False,
            "before_field_sha256": hashlib.sha256(old[field].encode()).hexdigest(),
            "after_field_sha256": hashlib.sha256(new[field].encode()).hexdigest(),
            "appended_text_sha256": hashlib.sha256(appended.encode()).hexdigest(),
            "appended_text": appended,
            "integrated_source": SOURCE,
            "original_append_is_verbatim_in_integrated_ledger": appended in note,
            "integrated_acceptance_paragraph": paragraphs[0],
            "interpretation": "原始作者阶段追加在本文件按原文保全；集成 ledger 使用审查后的明确接受记录。两者不是逐字相同，须人工判定采用。",
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=DIRECTORY.parents[2])
    parser.add_argument("--paths", type=int, help="print complete path groups for one non-ancestor PR")
    args = parser.parse_args()
    index = json.loads((DIRECTORY / "index.json").read_text())
    audit = AuditGit(args.repo)
    require(index["anchors"]["old_main"] == OLD_MAIN and
            index["anchors"]["integrated_source"] == SOURCE and
            index["anchors"]["integration_merge_sha"] == MERGE, "Anchor drift")
    require(audit.ancestor(OLD_MAIN, SOURCE), "Old main is not an integrated source ancestor")
    require(audit.lines("show", "-s", "--format=%P", MERGE) == [OLD_MAIN + " " + SOURCE],
            "PR142 merge parents differ from the audited history")
    rows = index["prs"]
    require([row["number"] for row in rows] == list(range(3, 142)), "Expected the complete 139-PR snapshot")
    counts = collections.Counter()
    for row in rows:
        evidence, category = audit.evidence(row)
        require(evidence == row["audit_evidence"], "Evidence drift for PR #" + str(row["number"]))
        require(category == row["recommended_class"], "Classification drift for PR #" + str(row["number"]))
        require(row["auto_close_eligible"] is (category in STRICT_CLASSES), "Eligibility drift")
        counts[category] += 1
    require(dict(counts) == index["counts"], "Class counts drift")
    require(audit.ledger_history() == json.loads((DIRECTORY / "pr139-ledger-history.json").read_text()),
            "Original PR139 ledger preservation drift")
    manual = next(row for row in rows if row["number"] == 139)
    _, manual_paths = audit.paths(manual)
    require(len(manual_paths["exact"]) == 40 and manual_paths["evolved"] == LEDGER_PATHS,
            "PR139 product/ledger boundary drift")
    require(manual["audit_evidence"]["unique_nonmerge_commits"] == 7 and
            manual["audit_evidence"]["matched_unique_nonmerge_commits"] == 6,
            "PR139 raw-delta adoption count drift")
    outcome = json.loads((DIRECTORY / "cleanup-outcome.json").read_text())
    require(outcome["merge_commit"] == MERGE and outcome["integrated_pull_request"] == 142 and
            outcome["original_open_count"] == 139, "Cleanup snapshot anchor drift")
    by_number = {row["number"]: row for row in rows}
    closed = outcome["results"]
    retained = outcome["retained_old_pull_requests"]
    expected_closed = {row["number"] for row in rows if row["recommended_class"] != "retain_requires_scope_review"}
    require(len(closed) == 93 and {row["number"] for row in closed} == expected_closed,
            "Cleanup result membership drift")
    require(len(retained) == 46 and {row["number"] for row in retained} == set(by_number) - expected_closed,
            "Retained PR snapshot membership drift")
    for entry in closed + retained:
        require(entry["head"] == by_number[entry["number"]]["head_sha"], "Cleanup head drift")
    require(collections.Counter(row["status"] for row in closed) == {"already_closed": 3, "closed": 90},
            "Cleanup status counts drift")
    require({row["number"] for row in closed if row["status"] == "already_closed"} == {5, 123, 131},
            "Already closed PR membership drift")
    require(outcome["cleanup_result"] == {"already_closed_by_integration": 3,
            "closed_after_fixed_head_recheck": 90, "failed": 0}, "Cleanup summary drift")
    result = {"status": "passed", "fixed_git_evidence_prs": len(rows), "counts": dict(counts),
              "cleanup_snapshot_consistency": {"closed": 93, "retained": 46},
              "ci_and_live_pr_states": "not queried or recertified by this local Git reproduction"}
    if args.paths is not None:
        selected = next((row for row in rows if row["number"] == args.paths), None)
        require(selected is not None, "PR is outside this audit snapshot")
        _, groups = audit.paths(selected)
        result["complete_path_groups"] = {"number": args.paths, "paths": groups}
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
