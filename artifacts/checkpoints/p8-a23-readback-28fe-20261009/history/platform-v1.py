"""Read original A23 platform/recovery/failure-gate evidence; never run products.

Public API: review_platform(audit_root: Path, source: Path, controller: Path).
Only trusted, pinned Python readers and Git source queries may execute.
Original ZIPs and extracted artifacts remain unchanged. All writes are in a
fresh derived directory. These results do not close roadmap dependencies.
"""
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import zipfile

CONFIG = json.loads(r'''{
  "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
  "source_tree": "58147c952505c44da1f41eb4b9c31643f2303b96",
  "source_inputs": 1087,
  "source_manifest_sha256": "4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00",
  "historical_commit": "277f2490fad3fa30f2812b5547bad033867c9ea5",
  "historical_tree": "992e3310f2e5595d53f66b05e9f1e8319273119e",
  "historical_inputs": 704,
  "historical_manifest_sha256": "aa075bde61e9aca29b43492bdf5d69009a7abc4fbde302eb7b48c40fa38d83a9",
  "archive_commit": "4630e635cd762bfbd2726cf1dd49e37803c06034",
  "artifacts": [
    {
      "id": 11591108235,
      "name": "p8-platform-cell-macos-1.95-semantic-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 8270537,
      "sha256": "f9709600c728281e9b1f81d0f50c0c6b2e844a497603a449a697d90f759bf6e9"
    },
    {
      "id": 11591159319,
      "name": "p8-platform-cell-macos-stable-default-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 8351887,
      "sha256": "b3ea19fa9254adaa4128616eaad557b3e55a36de3ab1563216e96ebd63623d44"
    },
    {
      "id": 11591229630,
      "name": "p8-platform-cell-macos-1.95-default-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 8031708,
      "sha256": "47953cf9b9a630f4b3b2e82dbae632f19c978403198aebb7595489573af37d9e"
    },
    {
      "id": 11591733636,
      "name": "p8-platform-cell-linux-1.95-default-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 8768177,
      "sha256": "a828b75ac08249d4cd888d8a14185684b7902842a4afad859a7cee57d507f23b"
    },
    {
      "id": 11592146562,
      "name": "p8-full-recovery-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 121408707,
      "sha256": "bf0209c994ed0d4688814342aecddb6045ed24bbf7cd54a27621e2d5cabd091e"
    },
    {
      "id": 11592180029,
      "name": "p8-gates-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838927,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 32985345,
      "sha256": "792a4fa9cfdcc3d9f1bf077a20e3d951958c144142fac1e81183aa132c4dbd25"
    },
    {
      "id": 11592442010,
      "name": "p8-platform-cell-linux-1.95-semantic-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 9022522,
      "sha256": "36fd353584c30d67c3da8b42ed16fdc8709b952a62acf63b562e17e1bbac3edd"
    },
    {
      "id": 11592465349,
      "name": "p8-platform-cell-macos-stable-semantic-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 8605542,
      "sha256": "83ffc0a9fbb2c8d4acac780ee427022e5999f2bf89734b539342e03ba602e174"
    },
    {
      "id": 11592498371,
      "name": "p8-platform-complete-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 1504,
      "sha256": "28a2590190645f2e923926097092d6008e3a8c7014dc78b3b832fb6608712700"
    },
    {
      "id": 11592750594,
      "name": "p8-platform-cell-linux-stable-default-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 8719713,
      "sha256": "e2d1aa0f541f9cb01899db37339381ae65e9b32df4f7fe094489fc3973790f73"
    },
    {
      "id": 11592990763,
      "name": "p8-platform-cell-linux-stable-semantic-a23bb72d3c954f385b99fe81ce9189885c208557",
      "run_id": 37871838952,
      "run_attempt": 1,
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "bytes": 8968832,
      "sha256": "cb2505655a8b3c72bd8caade9b291f05839819fc6b19f0232c36c08854ad8214"
    }
  ],
  "helpers": [
    {
      "path": "scripts/p7_build_identity.py",
      "sha256": "5003305105464936edf8f444fc0b93cd14a5d5f074dc650f16de2259e86f1cc5"
    },
    {
      "path": "scripts/p8_cold_build.py",
      "sha256": "f910326658b5539c539d1c02cd29a1236e11911b581f1beaf2495a42b53319fa"
    },
    {
      "path": "scripts/p8_rollback.py",
      "sha256": "4aa129fb9f2cd72b591d5c26a3b5e87aae0e2c3c6d03717c199fa612a322de50"
    },
    {
      "path": "scripts/p8_recovery.py",
      "sha256": "a290e87dcd1420ca37d4a32fc0e4ddbd2b269b407e8775a3de42c6cf7de61f42"
    },
    {
      "path": "scripts/p7_fault_lifecycle_stdio.py",
      "sha256": "0e1a0b6054ff5cb3708a7acfdc210af909849e62d8ff0616160e02e3988a2fdd"
    },
    {
      "path": "scripts/resource_harness/__init__.py",
      "sha256": "3193f3cffa59c2c6613b3ec007343207598b3cbb0d7c4c7fbcf4513cb9ca3a36"
    },
    {
      "path": "scripts/resource_harness/runtime.py",
      "sha256": "5876d2b0102f6e3a38189b2337c1783500393acde0dc04666d7f97ff34d92cc5"
    },
    {
      "path": "scripts/p8_gate_controls.py",
      "sha256": "0ea7c6544392b5cbe87fede2dc01ed0912e6da4ddc6f5ced621b9c595f1ec6f7"
    }
  ],
  "review_files": [
    {
      "path": "recovery-raw-event-audit.py",
      "archive_path": "review-platform/audit-scripts/recovery-raw-event-audit.py",
      "blob": "8da31c955be61dec5919f3ee4fa26ed11f1fe6d5",
      "bytes": 13332,
      "sha256": "8287c2df1c2dbc790abfe88706d36976ad7c7d8d20f723d2c86e901fe681db57"
    },
    {
      "path": "recovery-database-replay-v2.py",
      "archive_path": "review-platform/audit-scripts/recovery-database-replay-v2.py",
      "blob": "05e789dbe8d31d204d8a5b15cef3f8c447008168",
      "bytes": 9841,
      "sha256": "c8c05abcaca40c8f4858bf60c081dc0612801e841013518d27b35ee2649628d8"
    },
    {
      "path": "gates-original-case-audit.json",
      "archive_path": "review-platform/gates-original-case-audit.json",
      "blob": "ed5a936ed345090639d436c32d2f3a36acb6c71f",
      "bytes": 7515,
      "sha256": "70d30efa218bab09ab4cd35ca535a5ed6283070c40ccd7f7d95a42219b8242b0"
    },
    {
      "path": "gates-retained-cli/replay-report-v2.json",
      "archive_path": "review-platform/gates-retained-cli/replay-report-v2.json",
      "blob": "c8de66ae5509dbd279b5fe36a012b2f975cbc00c",
      "bytes": 75719,
      "sha256": "a8a8e4775981e2c188841abb236ca5dd3fd0e2ae371531f0ee0a80c7315d3841"
    },
    {
      "path": "gates-retained-cli/outputs-v2/bad_policy.json",
      "archive_path": "review-platform/gates-retained-cli/outputs-v2/bad_policy.json",
      "blob": "8c126c624d51cbbe1c2e8cdc7bccbe54a72aead4",
      "bytes": 550,
      "sha256": "951b5b7744e162c61437ab82819993c43c3e7b32cf2b09cae5b2920a2db41248"
    },
    {
      "path": "gates-retained-cli/outputs-v2/insufficient_samples.json",
      "archive_path": "review-platform/gates-retained-cli/outputs-v2/insufficient_samples.json",
      "blob": "0c06ad0cc2ad41b88dd2e32df9e1e5b12734a067",
      "bytes": 509,
      "sha256": "a0bba99b39f560b5f12385f8a6566cbadf0d88ade2e717c0cf0e160781eba549"
    },
    {
      "path": "gates-retained-cli/outputs-v2/latency_failed.json",
      "archive_path": "review-platform/gates-retained-cli/outputs-v2/latency_failed.json",
      "blob": "14451255ac01b0542a5b2b3b2a2e1da44a1d6bc2",
      "bytes": 492,
      "sha256": "e4e06446ee049ad6f32da94ef4cec7a9caa20389a6e33c51acd3ee88f6e8b1ec"
    },
    {
      "path": "gates-retained-cli/outputs-v2/quality_failed.json",
      "archive_path": "review-platform/gates-retained-cli/outputs-v2/quality_failed.json",
      "blob": "ee72b7ea9e18de18a3ede76bf4ad993d27a9ded0",
      "bytes": 520,
      "sha256": "3eb616998568a4b54efe4d6d6cbf401c60b4ed02d19780a1bd0d1f99e586d6a2"
    },
    {
      "path": "gates-retained-cli/outputs-v2/raw_lock_drift.json",
      "archive_path": "review-platform/gates-retained-cli/outputs-v2/raw_lock_drift.json",
      "blob": "ff0b4c6ac6cb01a914c2871a56e05c5b15b156ca",
      "bytes": 536,
      "sha256": "d8cfe3faef88fc15143b56497b13c64db746b5e4e863ac4ae0ccae245d5c69e6"
    },
    {
      "path": "gates-retained-cli/outputs-v2/zero_latency_denominator.json",
      "archive_path": "review-platform/gates-retained-cli/outputs-v2/zero_latency_denominator.json",
      "blob": "2026c2f27d2dc3dcfd7a1e86ea3b4d13217f5ae6",
      "bytes": 540,
      "sha256": "e4b10415f9b0b53ba465fb6da6f6a3aa7319dbaf5ebb109fc13b42ce65115f1a"
    },
    {
      "path": "gates-retained-cli/v2-bad_policy.stderr.log",
      "archive_path": "review-platform/gates-retained-cli/v2-bad_policy.stderr.log",
      "blob": "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
      "bytes": 0,
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    },
    {
      "path": "gates-retained-cli/v2-bad_policy.stdout.log",
      "archive_path": "review-platform/gates-retained-cli/v2-bad_policy.stdout.log",
      "blob": "4f4f5f15a94f5bc392bafee8d6fca72e45008839",
      "bytes": 22,
      "sha256": "c521e8291edf77abe7ddc18d94c69c9497d228d35b181acd82152b9843cf98e7"
    },
    {
      "path": "gates-retained-cli/v2-insufficient_samples.stderr.log",
      "archive_path": "review-platform/gates-retained-cli/v2-insufficient_samples.stderr.log",
      "blob": "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
      "bytes": 0,
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    },
    {
      "path": "gates-retained-cli/v2-insufficient_samples.stdout.log",
      "archive_path": "review-platform/gates-retained-cli/v2-insufficient_samples.stdout.log",
      "blob": "907597fa6a8b285c6676a58712ae20c23e93ae06",
      "bytes": 15,
      "sha256": "4a1c691f0a68de75465d44c6c26b75cbddf9128bce672bd5062db385e647b4cf"
    },
    {
      "path": "gates-retained-cli/v2-latency_failed.stderr.log",
      "archive_path": "review-platform/gates-retained-cli/v2-latency_failed.stderr.log",
      "blob": "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
      "bytes": 0,
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    },
    {
      "path": "gates-retained-cli/v2-latency_failed.stdout.log",
      "archive_path": "review-platform/gates-retained-cli/v2-latency_failed.stdout.log",
      "blob": "9a208e76291ab58a6142478ab520c7a1983c415e",
      "bytes": 9,
      "sha256": "e9c6c7eab63b97ace1ed584f168beaba80eb77c85e4aef9d014e7242bef77f8e"
    },
    {
      "path": "gates-retained-cli/v2-output_overwrite_refusal.stderr.log",
      "archive_path": "review-platform/gates-retained-cli/v2-output_overwrite_refusal.stderr.log",
      "blob": "c131aa6bb60a812e91da5596d927a9dffa8b3d53",
      "bytes": 31,
      "sha256": "64d53f380bdeff6002a944f419038625a0789b4100dd3ad8922f854229f9526c"
    },
    {
      "path": "gates-retained-cli/v2-output_overwrite_refusal.stdout.log",
      "archive_path": "review-platform/gates-retained-cli/v2-output_overwrite_refusal.stdout.log",
      "blob": "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
      "bytes": 0,
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    },
    {
      "path": "gates-retained-cli/v2-quality_failed.stderr.log",
      "archive_path": "review-platform/gates-retained-cli/v2-quality_failed.stderr.log",
      "blob": "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
      "bytes": 0,
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    },
    {
      "path": "gates-retained-cli/v2-quality_failed.stdout.log",
      "archive_path": "review-platform/gates-retained-cli/v2-quality_failed.stdout.log",
      "blob": "9a208e76291ab58a6142478ab520c7a1983c415e",
      "bytes": 9,
      "sha256": "e9c6c7eab63b97ace1ed584f168beaba80eb77c85e4aef9d014e7242bef77f8e"
    },
    {
      "path": "gates-retained-cli/v2-raw_lock_drift.stderr.log",
      "archive_path": "review-platform/gates-retained-cli/v2-raw_lock_drift.stderr.log",
      "blob": "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
      "bytes": 0,
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    },
    {
      "path": "gates-retained-cli/v2-raw_lock_drift.stdout.log",
      "archive_path": "review-platform/gates-retained-cli/v2-raw_lock_drift.stdout.log",
      "blob": "4f4f5f15a94f5bc392bafee8d6fca72e45008839",
      "bytes": 22,
      "sha256": "c521e8291edf77abe7ddc18d94c69c9497d228d35b181acd82152b9843cf98e7"
    },
    {
      "path": "gates-retained-cli/v2-zero_latency_denominator.stderr.log",
      "archive_path": "review-platform/gates-retained-cli/v2-zero_latency_denominator.stderr.log",
      "blob": "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
      "bytes": 0,
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    },
    {
      "path": "gates-retained-cli/v2-zero_latency_denominator.stdout.log",
      "archive_path": "review-platform/gates-retained-cli/v2-zero_latency_denominator.stdout.log",
      "blob": "907597fa6a8b285c6676a58712ae20c23e93ae06",
      "bytes": 15,
      "sha256": "4a1c691f0a68de75465d44c6c26b75cbddf9128bce672bd5062db385e647b4cf"
    }
  ],
  "platform_cells": [
    {
      "cell": {
        "package": "default",
        "platform": "linux",
        "toolchain": "1.95"
      },
      "id": 11591733636,
      "job": 113631480900
    },
    {
      "cell": {
        "package": "semantic",
        "platform": "linux",
        "toolchain": "1.95"
      },
      "id": 11592442010,
      "job": 113631480999
    },
    {
      "cell": {
        "package": "default",
        "platform": "linux",
        "toolchain": "stable"
      },
      "id": 11592750594,
      "job": 113631481002
    },
    {
      "cell": {
        "package": "semantic",
        "platform": "linux",
        "toolchain": "stable"
      },
      "id": 11592990763,
      "job": 113631481011
    },
    {
      "cell": {
        "package": "default",
        "platform": "macos",
        "toolchain": "1.95"
      },
      "id": 11591229630,
      "job": 113631481350
    },
    {
      "cell": {
        "package": "semantic",
        "platform": "macos",
        "toolchain": "1.95"
      },
      "id": 11591108235,
      "job": 113631481010
    },
    {
      "cell": {
        "package": "default",
        "platform": "macos",
        "toolchain": "stable"
      },
      "id": 11591159319,
      "job": 113631480988
    },
    {
      "cell": {
        "package": "semantic",
        "platform": "macos",
        "toolchain": "stable"
      },
      "id": 11592465349,
      "job": 113631481070
    }
  ],
  "products": {
    "default": {
      "status": "accepted_source_and_cargo_and_binary",
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "source_tree": "58147c952505c44da1f41eb4b9c31643f2303b96",
      "source_input_count": 1087,
      "manifest_sha256": "4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00",
      "binary_sha256": "685b592eacb7fc92e91f80644f6e9a9d14f2cd72924d49cdca362277a6e80b79",
      "binary_bytes": 67157480,
      "features": [],
      "cold_claim": false
    },
    "previous-default": {
      "status": "accepted_source_and_cargo_and_binary",
      "source_commit": "277f2490fad3fa30f2812b5547bad033867c9ea5",
      "source_tree": "992e3310f2e5595d53f66b05e9f1e8319273119e",
      "source_input_count": 704,
      "manifest_sha256": "aa075bde61e9aca29b43492bdf5d69009a7abc4fbde302eb7b48c40fa38d83a9",
      "binary_sha256": "6d2bccc1adbd277d4c40b64201df823bf302ca37bcb6a21b424e5a9530f8a5f6",
      "binary_bytes": 66413456,
      "features": [],
      "cold_claim": false
    },
    "semantic": {
      "status": "accepted_source_and_cargo_and_binary",
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "source_tree": "58147c952505c44da1f41eb4b9c31643f2303b96",
      "source_input_count": 1087,
      "manifest_sha256": "4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00",
      "binary_sha256": "bd45afaf4fb4720f74c31fe326fbe245507aa73977fc4cac0c0d9f9e7278f265",
      "binary_bytes": 69230272,
      "features": [
        "semantic"
      ],
      "cold_claim": false
    },
    "semantic-http": {
      "status": "accepted_source_and_cargo_and_binary",
      "source_commit": "a23bb72d3c954f385b99fe81ce9189885c208557",
      "source_tree": "58147c952505c44da1f41eb4b9c31643f2303b96",
      "source_input_count": 1087,
      "manifest_sha256": "4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00",
      "binary_sha256": "2a2c49b8e3c67331daa19dbe27cea266b1012b7c1038b4760098a729511cbc27",
      "binary_bytes": 78821416,
      "features": [
        "semantic",
        "semantic-http"
      ],
      "cold_claim": false
    }
  },
  "fault_tests": [
    {
      "package": "cc-semantic",
      "target": "p7_crash_preparation",
      "name": "isolated_sigkill_reopen_replay_preparation",
      "status": "accepted_original_exact_nonignored_test",
      "stdout_sha256": "9dc57394eed2792edb137977dfe333c08529a6d4fcda9c7e7c3030f765e1ec03",
      "executable_sha256": "4ba366d57bad9cc21a0e28b937a3b560f3dbbbf331321762bed16003ebf18a39",
      "exit_code": 0
    },
    {
      "package": "cc-semantic",
      "target": "crash_independent_review",
      "name": "independently_observed_sigkill_boundaries",
      "status": "accepted_original_exact_nonignored_test",
      "stdout_sha256": "7336e84533b802296ff207ed7ff5381fcb321bb71d77df12b3539b69bd3b7d9f",
      "executable_sha256": "68eabebbb35cfa5f8fddb80d92021d06a7b2d09965c1dd226863770e9898286a",
      "exit_code": 0
    },
    {
      "package": "cc-db",
      "target": null,
      "name": "index_db_rebuild::fault_tests::owned_process_kill_between_sidecar_cleanup_rename_and_reopen_preserves_database",
      "status": "accepted_original_exact_nonignored_test",
      "stdout_sha256": "cdd893aedc554a52f3b811da4505cf8cfd35f2d6aa4551c45f574435efd471fb",
      "executable_sha256": "d998ac3120159cd05f158a121bc5a305a5ab4c80df2149a679d48b7fcd3190f3",
      "exit_code": 0
    },
    {
      "package": "cc-db",
      "target": null,
      "name": "index_db_rebuild::fault_tests::writer_mutex_spans_rename_reopen_and_connection_installation",
      "status": "accepted_original_exact_nonignored_test",
      "stdout_sha256": "515e70d7d833122474c3176d60b2b2e1c20ffda4937028fda0d6c15a44b0bfcb",
      "executable_sha256": "d998ac3120159cd05f158a121bc5a305a5ab4c80df2149a679d48b7fcd3190f3",
      "exit_code": 0
    },
    {
      "package": "cc-semantic",
      "target": "artifact_cache",
      "name": "meta_tampering_is_detected_as_corrupt",
      "status": "accepted_original_exact_nonignored_test",
      "stdout_sha256": "20627d1e1d7ecbd143c7e0f7875928c0701103c7ac275fc4b180a4e9f2856747",
      "executable_sha256": "f05629c9f48b5e6ab2477fd2b1bd6ee68da7a5d2358d299eabd683cdfb545e55",
      "exit_code": 0
    },
    {
      "package": "cc-semantic",
      "target": "artifact_cache",
      "name": "different_spaces_never_cross_hit",
      "status": "accepted_original_exact_nonignored_test",
      "stdout_sha256": "bcc3530a79091d235c702027307b05da1f6851062d123c88255b8a9fe1fc4f41",
      "executable_sha256": "f05629c9f48b5e6ab2477fd2b1bd6ee68da7a5d2358d299eabd683cdfb545e55",
      "exit_code": 0
    },
    {
      "package": "cc-semantic",
      "target": "artifact_cache",
      "name": "namespaces_are_isolated",
      "status": "accepted_original_exact_nonignored_test",
      "stdout_sha256": "38cc2880797f7d021e03dc277c5922f0f4041f8eb443ed2936c155eafa775305",
      "executable_sha256": "f05629c9f48b5e6ab2477fd2b1bd6ee68da7a5d2358d299eabd683cdfb545e55",
      "exit_code": 0
    }
  ]
}''')

ORIGINAL_ROOT = "/home/runner/work/codecortex/codecortex"
ORIGINAL_RECOVERY = "/home/runner/work/_temp/p8-full-recovery"
ORIGINAL_GATES = "/home/runner/work/_temp/p8-gates"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, "duplicate JSON key: " + key)
        value[key] = item
    return value


def bad_constant(value):
    raise ValueError("nonfinite JSON number: " + value)


def finite_float(value):
    number = float(value)
    require(math.isfinite(number), "nonfinite JSON float")
    return number


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8"),
                      object_pairs_hook=unique_object, parse_constant=bad_constant, parse_float=finite_float)


def write_new(path, value):
    with Path(path).open("x", encoding="utf-8") as stream:
        json.dump(value, stream, sort_keys=True, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write("\n")


def safe_name(name):
    require(isinstance(name, str) and name and "\\" not in name and "\0" not in name,
            "invalid evidence member name")
    path = PurePosixPath(name)
    require(not path.is_absolute() and ".." not in path.parts and str(path) == name,
            "noncanonical evidence member name")
    return name


def file_map(root, excluded=()):
    root = Path(root)
    require(root.is_dir() and not root.is_symlink(), "evidence root is not a regular directory")
    values = {}
    for directory, folders, files in os.walk(root, followlinks=False):
        here = Path(directory)
        for name in folders:
            require(not (here / name).is_symlink(), "symlink evidence directory")
        for name in files:
            path = here / name
            require(stat.S_ISREG(path.lstat().st_mode) and not path.is_symlink(),
                    "nonregular evidence member")
            relative = safe_name(path.relative_to(root).as_posix())
            if relative not in excluded:
                values[relative] = digest(path)
    return values


def assert_files(root, expected, excluded=()):
    require(isinstance(expected, dict), "sealed inventory is not a digest map")
    for name, sha in expected.items():
        safe_name(name)
        require(isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{64}", sha),
                "invalid sealed digest")
    actual = file_map(root, excluded)
    require(actual == expected, "sealed evidence inventory or bytes differ")
    return actual


def recorded_path(value):
    require(isinstance(value, str) and value and "\\" not in value and "\0" not in value,
            "invalid producer path")
    path = PurePosixPath(value)
    require(path.is_absolute() and ".." not in path.parts and str(path) == value,
            "noncanonical producer path")
    return path


def finite_number(value):
    return type(value) in (int, float) and math.isfinite(value)


class Readback:
    def __init__(self, audit_root, source, controller):
        self.audit = Path(audit_root).resolve(strict=True)
        self.source = Path(source).resolve(strict=True)
        self.controller = Path(controller).resolve(strict=True)
        self.work = self.audit / "derived" / "platform-readback"
        self.work.parent.mkdir(exist_ok=True)
        self.work.mkdir()
        (self.work / "review-platform").mkdir()
        (self.work / "python-cache").mkdir()
        (self.work / "source").symlink_to(self.source, target_is_directory=True)
        (self.work / "raw").symlink_to(self.audit / "raw", target_is_directory=True)
        self.checked_artifacts = {}
        self.source_proof = None
        self.recovery_home = None
        self.recovery_report = None
        self.groups = {}

    def python_read(self, label, arguments, timeout=300):
        """Execute system Python readers only; arguments never contain artifact code."""
        stdout = self.work / (label + ".stdout.log")
        stderr = self.work / (label + ".stderr.log")
        command = [sys.executable, "-I", "-B", "-X",
                   "pycache_prefix=" + str(self.work / "python-cache"), *map(str, arguments)]
        observation = {"command": command, "operation": "read_only_python_evidence_reader",
                       "native_product_execution": False}
        try:
            with stdout.open("xb") as out, stderr.open("xb") as err:
                process = subprocess.run(command, cwd=self.work, stdout=out, stderr=err,
                                         timeout=timeout, check=False)
            observation["exit_code"] = process.returncode
        except subprocess.TimeoutExpired:
            observation.update(exit_code=None, error="read_only_reader_timeout")
            raise
        finally:
            observation["stdout_sha256"] = digest(stdout) if stdout.exists() else None
            observation["stderr_sha256"] = digest(stderr) if stderr.exists() else None
            write_new(self.work / (label + ".process.json"), observation)
        require(process.returncode == 0, label + " reader failed; original output retained")
        return stdout

    def fixed_review_file(self, name):
        row = next((x for x in CONFIG["review_files"] if x["path"] == name), None)
        require(row is not None, "unregistered review dependency")
        path = self.controller / "reviewers" / safe_name(name)
        require(path.resolve(strict=True) == path and stat.S_ISREG(path.lstat().st_mode),
                "nonregular review dependency")
        require(path.stat().st_size == row["bytes"] and digest(path) == row["sha256"],
                "pinned review dependency differs")
        return path

    def artifact(self, artifact_id):
        if artifact_id in self.checked_artifacts:
            return self.checked_artifacts[artifact_id]
        row = next(x for x in CONFIG["artifacts"] if x["id"] == artifact_id)
        directory = self.audit / "raw" / str(artifact_id)
        original = directory / "original.zip"
        home = directory / "extracted"
        require(original.resolve(strict=True) == original and original.is_file(),
                "original ZIP path is not owned")
        require(original.stat().st_size == row["bytes"] and digest(original) == row["sha256"],
                "original ZIP differs from fixed official identity")
        require(home.resolve(strict=True) == home, "extracted root is not owned")
        actual = file_map(home)
        seen = {}
        with zipfile.ZipFile(original) as archive:
            for info in archive.infolist():
                if info.is_dir():
                    safe_name(info.filename.rstrip("/"))
                    with archive.open(info) as stream:
                        require(stream.read() == b"", "nonempty ZIP directory")
                    continue
                name = safe_name(info.filename)
                require(name not in seen and not stat.S_ISLNK(info.external_attr >> 16),
                        "duplicate or symlink ZIP member")
                value, count = hashlib.sha256(), 0
                with archive.open(info) as stream:
                    for block in iter(lambda: stream.read(1024 * 1024), b""):
                        value.update(block)
                        count += len(block)
                require(count == info.file_size, "ZIP member length differs")
                seen[name] = value.hexdigest()
        require(actual == seen, "extracted bytes do not equal complete original ZIP")
        result = {"home": home, "identity": row, "members": len(seen),
                  "files": seen, "zip_sha256": row["sha256"]}
        self.checked_artifacts[artifact_id] = result
        return result

    def require_source(self):
        require(self.source_proof is not None, "exact A23 source proof unavailable")
        return self.source_proof

    def source_group(self):
        observed = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.source,
                                           text=True).strip()
        require(observed == CONFIG["source_commit"], "source HEAD differs from A23")
        for row in CONFIG["helpers"]:
            path = self.source / row["path"]
            require(path.resolve(strict=True) == path and path.is_file()
                    and digest(path) == row["sha256"], "A23 helper bytes differ")
        # Isolated readers import only the committed Python tree. -B plus the
        # fresh pycache prefix prevents preexisting timestamp-valid pyc reuse.
        listing = subprocess.check_output(["git", "ls-tree", "-r", "-z", observed,
                                           "--", "scripts"], cwd=self.source)
        expected = {}
        for raw in listing.split(b"\0"):
            if not raw:
                continue
            metadata, raw_name = raw.split(b"\t", 1)
            mode, kind, oid = metadata.decode().split()
            name = raw_name.decode("utf-8")
            if not name.endswith(".py"):
                continue
            require(kind == "blob" and mode in ("100644", "100755"),
                    "nonregular committed Python observer")
            path = self.source / name
            require(path.resolve(strict=True) == path and stat.S_ISREG(path.lstat().st_mode),
                    "nonregular loaded observer")
            body = path.read_bytes()
            require(hashlib.sha1(b"blob " + str(len(body)).encode() + b"\0" + body).hexdigest() == oid,
                    "committed Python observer bytes differ")
            expected[name] = oid
        actual = {p.relative_to(self.source).as_posix()
                  for p in (self.source / "scripts").rglob("*.py")}
        require(actual == set(expected), "unbound Python observer module")
        program = (
            "import json,sys; from pathlib import Path; "
            "root=Path(sys.argv[1]); sys.path.insert(0,str(root/'scripts')); "
            "from p7_build_identity import source_snapshot; "
            "from p8_cold_build import observer_snapshot; "
            "print(json.dumps({'source':source_snapshot(root),"
            "'observer':observer_snapshot(root,'full')},sort_keys=True))"
        )
        path = self.python_read("source-proof", ["-c", program, self.source])
        proof = read_json(path)
        source = proof["source"]
        require(source["source_commit"] == CONFIG["source_commit"]
                and source["source_tree"] == CONFIG["source_tree"]
                and source["input_count"] == CONFIG["source_inputs"]
                and source["manifest_sha256"] == CONFIG["source_manifest_sha256"],
                "complete A23 Cargo/crate source identity differs")
        require(len(source["inputs"]) == CONFIG["source_inputs"], "source denominator differs")
        self.source_proof = proof
        return {key: value for key, value in source.items() if key != "inputs"}

    def platform_group(self):
        self.require_source()
        stage = self.work / "platform-cells"
        stage.mkdir()
        for cell in CONFIG["platform_cells"]:
            item = self.artifact(cell["id"])
            require(item["members"] == 19, "original cell member inventory differs")
            destination = stage / str(cell["id"])
            shutil.copytree(item["home"], destination)
            require(file_map(destination) == item["files"], "copied original bundle differs")
        collector = self.artifact(11592498371)
        require(set(collector["files"]) == {"matrix.json"}, "original collector inventory differs")
        original = read_json(collector["home"] / "matrix.json")
        output = self.work / "platform-collector"
        program = (
            "import sys; from pathlib import Path; root=Path(sys.argv[1]); "
            "sys.path.insert(0,str(root/'scripts')); from p8_cold_build import main; "
            "raise SystemExit(main(['--source-root',str(root),'--collect-cells',sys.argv[2],"
            "'--expected-commit',sys.argv[3],'--output-dir',sys.argv[4]]))"
        )
        self.python_read("platform-collector",
                         ["-c", program, self.source, stage, CONFIG["source_commit"], output])
        replayed = read_json(output / "matrix.json")
        require(original["source"]["source_root"] == ORIGINAL_ROOT
                and original["input_directory"] == "/home/runner/work/_temp/p8-platform-cells",
                "original collector producer paths differ")
        normalized = dict(original, input_directory=str(stage))
        normalized["source"] = dict(original["source"], source_root=str(self.source))
        require(normalized == replayed, "original collector replay changed more than transport paths")
        require(replayed["status"] == "passed" and replayed["exit_code"] == 0
                and replayed["counts"] == {"passed": 8, "failed": 0, "not_run": 0},
                "complete original eight-cell collector refused")
        return {"original_artifact_id": 11592498371, "cells": replayed["cells"],
                "counts": replayed["counts"],
                "original_matrix_sha256": digest(collector["home"] / "matrix.json"),
                "replayed_matrix_sha256": digest(output / "matrix.json"),
                "executed_original_portable_collector": True,
                "fresh_build_or_native_smoke_rerun": False,
                "scope": replayed["scope"]}

    def command_record(self, home, expected, command):
        record = read_json(home / "command.json")
        require(record == expected and record["command"] == command, "original command identity differs")
        require(record["status"] == "passed" and type(record.get("exit_code")) is int
                and record["exit_code"] == 0 and "error" not in record,
                "original command did not complete successfully")
        require(type(record.get("pid")) is int and record["pid"] > 0
                and finite_number(record.get("wall_seconds")) and record["wall_seconds"] >= 0,
                "actual process identity or duration is missing")
        require(set(record["logs_sha256"]) == {"stdout.log", "stderr.log"},
                "both original command logs are required")
        for name, sha in record["logs_sha256"].items():
            require(digest(home / name) == sha, "original command log differs")
        return record

    def cargo_rows(self, path):
        rows = [json.loads(line, object_pairs_hook=unique_object, parse_constant=bad_constant, parse_float=finite_float)
                for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
        require(any(r.get("reason") == "build-finished" and r.get("success") is True for r in rows)
                and not any(r.get("reason") == "build-finished" and r.get("success") is not True
                            for r in rows), "original Cargo completion missing or failed")
        return rows

    def test_record(self, home, record, expected, original_parent):
        source = self.require_source()["source"]
        require(record == read_json(home / "test-receipt.json"),
                "test summary differs from original receipt")
        package, target, name = expected["package"], expected["target"], expected["test_name"]
        require(record["status"] == "passed" and record["package"] == package
                and record["target"] == target and record["test_name"] == name
                and record["source"] == source, "original test/source identity differs")
        native = original_parent + "/fault-test"
        require(record["retained_executable"] == native
                and digest(home / "fault-test") == record["executable_sha256"]
                == expected["executable_sha256"], "retained original test binary differs")
        command = record["build"]["command"]
        recorded_path(command[0])
        wanted = ["test", "-p", package, "--locked", "--offline", "--no-run",
                  "--message-format=json-render-diagnostics"]
        wanted.extend(["--test", target] if target else ["--lib"])
        require(command[1:] == wanted, "original test build command differs")
        self.command_record(home / "build", record["build"], command)
        argv = [native, "--exact", name, "--nocapture", "--test-threads=1"]
        self.command_record(home / "execution", record["execution"], argv)
        raw = (home / "execution/stdout.log").read_text(encoding="utf-8")
        require(digest(home / "execution/stdout.log") == expected["stdout_sha256"],
                "fixed original test output differs")
        require(re.search(r"test result: ok\. 1 passed; 0 failed; 0 ignored;", raw) is not None
                and len(re.findall(r"test result:", raw)) == 1,
                "original test was not exactly one passing nonignored control")
        rows = self.cargo_rows(home / "build/stdout.log")
        target_name = target or package.replace("-", "_")
        artifacts = [r for r in rows if r.get("reason") == "compiler-artifact"
                     and r.get("target", {}).get("name") == target_name and r.get("executable")
                     and r.get("profile", {}).get("test") is True]
        require(artifacts == [record["cargo_artifact"]], "original exact test Cargo artifact differs")
        artifact = artifacts[0]
        require(artifact["manifest_path"] == ORIGINAL_ROOT + "/crates/" + package + "/Cargo.toml",
                "original test compiled from another checkout")
        root = recorded_path(artifact["executable"])
        target_root = PurePosixPath("/home/runner/work/_temp/p8-gates-target"
                                   if original_parent.startswith(ORIGINAL_GATES + "/")
                                   else ORIGINAL_RECOVERY + "/cargo-target")
        require(root.is_relative_to(target_root), "original test executable escaped its target")
        return {"test_name": name, "package": package, "target": target,
                "original_exit_code": record["execution"]["exit_code"],
                "original_pid": record["execution"]["pid"],
                "original_stdout_sha256": digest(home / "execution/stdout.log"),
                "binary_sha256": record["executable_sha256"], "new_execution": False}

    def recovery_inputs(self):
        self.require_source()
        if self.recovery_report is None:
            artifact = self.artifact(11592146562)
            self.recovery_home = artifact["home"] / "p8-full-recovery"
            self.recovery_report = read_json(self.recovery_home / "full-recovery.json")
        return self.recovery_home, self.recovery_report

    def recovery_product(self, home, record, key):
        expected = CONFIG["products"][key]
        package = "default" if key == "previous-default" else key
        directory = "product-previous" if key == "previous-default" else "product-" + key
        product = home / directory
        require(record == read_json(product / "build-receipt.json"), "original product receipt differs")
        before = record["source_before"]
        require(before == record["source_after"] and record["status"] == "passed"
                and record["build_exit_code"] == 0 and record["stop_reason"] is None
                and record["package_kind"] == package, "original product build failed or changed")
        source_root = ORIGINAL_ROOT + ("/p8-previous-source" if key == "previous-default" else "")
        require(before["source_root"] == source_root
                and before["source_commit"] == expected["source_commit"]
                and before["source_tree"] == expected["source_tree"]
                and before["input_count"] == expected["source_input_count"]
                and before["manifest_sha256"] == expected["manifest_sha256"],
                "current/historical source identity changed or was relabeled")
        require(record["source_manifest"] == "source-inputs.json", "product manifest path differs")
        manifest_path = product / "source-inputs.json"
        manifest = read_json(manifest_path)
        require(isinstance(manifest, dict) and len(manifest) == expected["source_input_count"]
                and digest(manifest_path) == expected["manifest_sha256"],
                "complete original product manifest differs")
        if key != "previous-default":
            require(manifest == self.source_proof["source"]["inputs"], "A23 product source map differs")
        for name, sha in manifest.items():
            safe_name(name)
            require(re.fullmatch(r"[0-9a-f]{64}", sha) is not None, "invalid source digest")
        binary = product / "codecortex"
        require(record["binary_path"] == ORIGINAL_RECOVERY + "/" + directory + "/codecortex"
                and record["binary_sha256"] == expected["binary_sha256"] == digest(binary)
                and record["binary_bytes"] == expected["binary_bytes"] == binary.stat().st_size,
                "retained original product binary differs")
        require(record["cold_build_claim"] is False and record["release_certified"] is False
                and record["build_profile"] == "dev", "engineering product scope changed")
        command = record["build_command"]
        recorded_path(command[0])
        wanted = ["build", "-p", "cc-server", "--bin", "codecortex", "--locked", "--offline",
                  "--no-default-features", "--message-format=json-render-diagnostics"]
        if package != "default":
            wanted += ["--features", package]
        require(command[1:] == wanted, "product original Cargo arguments differ")
        self.command_record(product / "build", record["build_observation"], command)
        rows = self.cargo_rows(product / "build/stdout.log")
        artifacts = [r for r in rows if r.get("reason") == "compiler-artifact"
                     and r.get("target", {}).get("name") == "codecortex" and r.get("executable")]
        require(artifacts == [record["cargo_artifact"]], "original product Cargo event differs")
        artifact = artifacts[0]
        target_root = ORIGINAL_RECOVERY + "/cargo-target"
        if key == "previous-default":
            target_root += "/previous-source"
        executable = target_root + "/debug/codecortex"
        profile = artifact["profile"]
        require(record["target_directory"] == target_root and artifact["executable"] == executable
                and artifact["manifest_path"] == source_root + "/crates/cc-server/Cargo.toml"
                and artifact["target"]["src_path"] == source_root + "/crates/cc-server/src/main.rs"
                and artifact["target"]["kind"] == ["bin"]
                and sorted(artifact["features"]) == expected["features"]
                and profile["test"] is False and profile["opt_level"] == "0"
                and profile["debug_assertions"] is True, "original product compiler identity differs")
        require(record["copy_source"] == {"path": executable, "bytes": binary.stat().st_size,
                                          "sha256": digest(binary)}, "original executable copy differs")
        return {"source_commit": before["source_commit"], "input_count": len(manifest),
                "binary_sha256": digest(binary), "features": artifact["features"],
                "actual_profile": "dev", "cold_build_claim": False, "release_certified": False}

    def recovery_identity_group(self):
        home, report = self.recovery_inputs()
        require(report["schema_version"] == 2
                and report["observer_binding"] == "required_git_and_loaded_source"
                and report["status"] == "passed_declared_fault_matrix"
                and report["source"] == self.source_proof["source"],
                "full original recovery/source receipt differs")
        sealed = assert_files(home, report["evidence_files_sha256"], {"full-recovery.json"})
        require(len(sealed) == 495 and not (home / "cargo-target").exists(),
                "full original 495-member evidence inventory differs")
        before = report["observer_before"]
        require(before == report["observer_after"], "original observer changed")
        portable = dict(before, source_root=str(self.source))
        require(portable == self.source_proof["observer"] and before["source_root"] == ORIGINAL_ROOT,
                "original seven observers differ from exact A23")
        require(len(before["inputs"]) == 7, "full observer inventory is incomplete")
        archived = file_map(home / "observer-source")
        require(set(archived) == set(before["inputs"]), "observer archive membership differs")
        for name, row in before["inputs"].items():
            path = home / "observer-source" / safe_name(name)
            require(archived[name] == row["sha256"] and path.stat().st_size == row["bytes"],
                    "original archived observer bytes differ")
        expected_runners = {"p8_recovery.py", "p8_rollback.py", "p8_cold_build.py",
                            "p7_build_identity.py", "p7_fault_lifecycle_stdio.py",
                            "resource_harness/runtime.py"}
        require(set(report["runner_files_sha256"]) == expected_runners, "runner inventory differs")
        for name, sha in report["runner_files_sha256"].items():
            require(digest(home / "runner-source" / name) == sha
                    == digest(self.source / "scripts" / name), "original runner source differs")
        require(set(report["products"]) == set(CONFIG["products"]), "original four product roles differ")
        products = {key: self.recovery_product(home, record, key)
                    for key, record in report["products"].items()}
        expected_tests = CONFIG["fault_tests"]
        require(len(report["executions"]) == len(expected_tests) == 7,
                "original production test denominator differs")
        tests = []
        for number, (record, expected) in enumerate(zip(report["executions"], expected_tests)):
            wanted = dict(expected, test_name=expected["name"])
            tests.append(self.test_record(home / ("fault-test-%02d" % number), record, wanted,
                                          ORIGINAL_RECOVERY + "/fault-test-%02d" % number))
        require(report["executed_active_seeds"] == [223, 227, 229]
                and [row["seed"] for row in report["active_stdio"]] == [223, 227, 229],
                "original active seed denominator differs")
        require(report["release_certified"] is False and report["actual_paid_cost"] is None,
                "original recovery scope/unknown paid cost changed")
        return {"original_sealed_files": len(sealed), "products": products,
                "original_production_tests": tests, "active_seeds": [223, 227, 229],
                "actual_paid_cost": None, "limitations": report["limitations"],
                "new_fault_or_native_execution": False}

    def recovery_events_group(self):
        self.recovery_inputs()
        script = self.fixed_review_file("recovery-raw-event-audit.py")
        self.python_read("recovery-events", [script])
        path = self.work / "review-platform/recovery-raw-event-audit.json"
        result = read_json(path)
        require(result["status"] == "raw_RPC_HTTP_lifecycle_accepted_database_backup_replay_pending"
                and result["source_commit"] == CONFIG["source_commit"]
                and len(result["native_sessions"]) == 13 and len(result["local_cases"]) == 3
                and [row["seed"] for row in result["active_seeds"]] == [223, 227, 229],
                "original raw event reader did not verify complete domains")
        return {"original_reader_report": result, "report_sha256": digest(path),
                "scope": "original 13 sessions and 3 local + 3x4 active cases; no new fault run"}

    def recovery_databases_group(self):
        self.recovery_inputs()
        script = self.fixed_review_file("recovery-database-replay-v2.py")
        self.python_read("recovery-databases", [script])
        path = self.work / "review-platform/recovery-database-rollback-audit.json"
        result = read_json(path)
        require(result["status"] == "accepted_original_database_config_and_version_rollback"
                and result["source_commit"] == CONFIG["source_commit"]
                and result["historical_source_commit"] == CONFIG["historical_commit"]
                and len(result["database_copies"]) == 14
                and result["rollback_schemas"] == [25, 1025, 25, 25]
                and result["actual_historical_schemas"] == [25, 24, 25, 25]
                and result["inactive_cache_reader_not_run_preserved"] is True,
                "original database/rollback readback is incomplete")
        return {"original_reader_report": result, "report_sha256": digest(path),
                "scope": "14 owned byte copies, SELECT/PRAGMA only, ro immutable after empty-WAL check",
                "new_backup_restore_gc_or_fault_run": False}

    def gates_group(self):
        source = self.require_source()["source"]
        artifact = self.artifact(11592180029)
        home = artifact["home"]
        report = read_json(home / "report.json")
        require(report["status"] == "passed_original_failure_gates" and report["exit_code"] == 0
                and report["source"] == source and report["release_approval"] is False
                and report["task_complete"] is False, "original failure-gate scope/source differs")
        assert_files(home, report["files"], {"report.json"})
        require(artifact["members"] == 337 and len(report["files"]) == 336,
                "original failure-gate inventory differs")
        original_audit = read_json(self.fixed_review_file("gates-original-case-audit.json"))
        require(original_audit["source_commit"] == CONFIG["source_commit"]
                and original_audit["source_tree"] == CONFIG["source_tree"]
                and original_audit["source_manifest_sha256"] == CONFIG["source_manifest_sha256"],
                "fixed original case audit source differs")
        require(len(report["cases"]) == len(original_audit["cases"]) == 6,
                "original six-control denominator differs")
        results = []
        for number, (record, fixed) in enumerate(zip(report["cases"], original_audit["cases"])):
            expected = {"package": "cc-eval", "target": "benchmark_cli", "test_name": fixed["test"],
                        "executable_sha256": fixed["test_binary_sha256"],
                        "stdout_sha256": fixed["test_stdout_sha256"]}
            results.append(self.test_record(home / ("case-%02d" % number), record, expected,
                                           ORIGINAL_GATES + "/case-%02d" % number))
        cli = report["cli"]
        binary = home / "cc-eval"
        require(digest(binary) == cli["executable_sha256"]
                == original_audit["retained_cli_sha256"]
                == "daac7973861d3f7a6a419dfa34a2563af5796a227b016903a5b91c30a793f80a",
                "original retained CLI binary differs")
        build = cli["cargo_artifact"]
        require(build["reason"] == "compiler-artifact" and build["target"]["name"] == "cc-eval"
                and build["target"]["kind"] == ["bin"]
                and build["manifest_path"] == ORIGINAL_ROOT + "/crates/cc-eval/Cargo.toml"
                and build["target"]["src_path"] == ORIGINAL_ROOT + "/crates/cc-eval/src/main.rs"
                and build["executable"] == cli["path"]
                == "/home/runner/work/_temp/p8-gates-target/debug/cc-eval",
                "retained CLI Cargo provenance differs")
        first_rows = self.cargo_rows(home / "case-00/build/stdout.log")
        require(build in first_rows, "retained CLI is not in original first build log")
        replay = read_json(self.fixed_review_file("gates-retained-cli/replay-report-v2.json"))
        require(replay["status"] == "accepted_all_7_original_retained_CLI_replays"
                and replay["source_commit"] == CONFIG["source_commit"]
                and replay["artifact_id"] == 11592180029
                and replay["artifact_zip_sha256"] == artifact["zip_sha256"]
                and replay["retained_cli_sha256"] == digest(binary)
                and replay["retained_cli_bytes"] == binary.stat().st_size
                and replay["original_artifact_files_unchanged"] == 337,
                "retained replay does not describe this exact original artifact")
        require(replay["image_reference"] ==
                "ubuntu@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b"
                and replay["image_architecture"] == "amd64" and replay["network"] == "none"
                and replay["original_artifact_mount"] == "read-only",
                "original CLI compatibility execution scope differs")
        fixtures = replay["working_fixture_manifest"]
        require(len(fixtures) == replay["working_fixture_files_before_after_equal"] == 287,
                "retained fixture denominator differs")
        fixture_bytes = 0
        for name, sha in fixtures.items():
            path = home / safe_name(name)
            require(path.is_file() and digest(path) == sha, "original retained replay fixture differs")
            fixture_bytes += path.stat().st_size
        require(fixture_bytes == replay["working_fixture_bytes"] == 209265,
                "retained fixture byte count differs")
        cases = [("quality_failed", 1, "failed"), ("latency_failed", 1, "failed"),
                 ("insufficient_samples", 1, "inconclusive"),
                 ("zero_latency_denominator", 1, "inconclusive"),
                 ("raw_lock_drift", 2, "invalid_measurement"),
                 ("bad_policy", 2, "invalid_measurement"),
                 ("output_overwrite_refusal", 2, None)]
        require([r["case"] for r in replay["cases"]] == [x[0] for x in cases],
                "retained seven-CLI denominator or order differs")
        observed = []
        for row, (name, expected_exit, expected_status) in zip(replay["cases"], cases):
            require(type(row["actual_exit_code"]) is int
                    and row["actual_exit_code"] == row["expected_exit_code"] == expected_exit,
                    "retained CLI failure/inconclusive exit was changed")
            for kind in ("stdout", "stderr"):
                path = self.fixed_review_file("gates-retained-cli/v2-" + name + "." + kind + ".log")
                require(digest(path) == row[kind + "_sha256"], "actual CLI process log differs")
            if expected_status is not None:
                output = self.fixed_review_file("gates-retained-cli/outputs-v2/" + name + ".json")
                data = read_json(output)
                require(data == row["actual_report"] and digest(output) == row["output_sha256"]
                        and data["status"] == row["actual_status"] == row["expected_status"] == expected_status
                        and data["exit_code"] == expected_exit, "actual CLI output contract differs")
                if name == "quality_failed":
                    require(data["p95_ratio"] == 1 and data["top1_delta"] == -1
                            and data["reasons"] == ["Top-1 regression", "nDCG regression"],
                            "quality failure raw result differs")
                elif name == "latency_failed":
                    require(data["p95_ratio"] == 10 and data["reasons"] == ["p95 regression"],
                            "latency failure raw result differs")
                elif name == "insufficient_samples":
                    require(data["baseline_samples"] == data["candidate_samples"] == 2
                            and data["inconclusive_reasons"] == ["insufficient latency samples"],
                            "insufficient-sample inconclusive result differs")
                elif name == "zero_latency_denominator":
                    require(data["baseline_samples"] == data["candidate_samples"] == 30
                            and data["p95_ratio"] is None
                            and data["inconclusive_reasons"] ==
                            ["p95 latency ratio unavailable (missing or zero baseline)"],
                            "zero-denominator inconclusive result differs")
                elif name == "raw_lock_drift":
                    require(data["reasons"] == ["invalid benchmark input: raw response drift"],
                            "raw-lock drift failure differs")
                elif name == "bad_policy":
                    require(data["reasons"] == ["JSON: expected value at line 1 column 1"],
                            "bad-policy failure differs")
            else:
                output = self.fixed_review_file("gates-retained-cli/outputs-v2/bad_policy.json")
                require(row["existing_report_unchanged"] is True
                        and row["output_before_sha256"] == row["output_after_sha256"] == digest(output)
                        and row["stderr"] == "I/O: File exists (os error 17)\n",
                        "original output overwrite refusal changed or lost raw")
            observed.append({"case": name, "original_exit_code": expected_exit,
                             "original_status": expected_status, "new_execution": False})
        return {"original_rust_controls": results, "original_cli_replays": observed,
                "retained_cli_sha256": digest(binary),
                "zero_plan_scope": replay["zero_plan_control"],
                "previous_transport_failure": replay["previous_transport_failure"],
                "original_execution_context": replay["execution_context"],
                "new_cli_docker_native_execution": False,
                "quality_performance_or_release_certification": False}

    def final_source_group(self):
        proof = self.require_source()
        require(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.source,
                                        text=True).strip() == CONFIG["source_commit"],
                "source HEAD changed during readback")
        for name, sha in proof["source"]["inputs"].items():
            require(digest(self.source / name) == sha, "source changed during readback")
        for row in CONFIG["helpers"]:
            require(digest(self.source / row["path"]) == row["sha256"],
                    "reader source changed during readback")
        return {"source_commit": CONFIG["source_commit"], "source_inputs_unchanged": True}

    def originals_after_group(self):
        verified = []
        for artifact_id, item in sorted(self.checked_artifacts.items()):
            original = self.audit / "raw" / str(artifact_id) / "original.zip"
            require(digest(original) == item["zip_sha256"], "original ZIP changed during readback")
            require(file_map(item["home"]) == item["files"],
                    "original extracted evidence changed during readback")
            verified.append(artifact_id)
        require(len(verified) == 11, "some original artifacts were not fully received")
        return {"original_artifact_ids": verified, "all_original_bytes_unchanged": True}

    def run_group(self, name, function):
        try:
            detail = function()
            row = {"status": "passed_readback", "detail": detail}
        except Exception as error:
            row = {"status": "failed_readback", "error_type": type(error).__name__,
                   "error": str(error), "waived": False}
        self.groups[name] = row
        try:
            write_new(self.work / (name + ".json"), row)
        except Exception as error:
            row["receipt_write_error"] = type(error).__name__ + ": " + str(error)
            row["status"] = "failed_readback"

    def run(self):
        for name, function in [
            ("source", self.source_group),
            ("platform", self.platform_group),
            ("recovery_identity", self.recovery_identity_group),
            ("recovery_raw_events", self.recovery_events_group),
            ("recovery_databases", self.recovery_databases_group),
            ("failure_gates", self.gates_group),
            ("source_after", self.final_source_group),
            ("originals_after", self.originals_after_group),
        ]:
            self.run_group(name, function)
        accepted = all(row["status"] == "passed_readback" for row in self.groups.values())
        result = {
            "schema": "a23-platform-readback-v1",
            "source_commit": CONFIG["source_commit"],
            "source_tree": CONFIG["source_tree"],
            "archive_commit": CONFIG["archive_commit"],
            "status": "passed_readback" if accepted else "failed_readback",
            "accepted": accepted,
            "groups": self.groups,
            "fixed_original_artifacts": CONFIG["artifacts"],
            "checked_original_artifacts": [
                {"id": key, "sha256": item["zip_sha256"], "members": item["members"]}
                for key, item in sorted(self.checked_artifacts.items())
            ],
            "derived_directory": str(self.work),
            "new_native_measurements": 0,
            "new_product_or_fault_executions": 0,
            "new_retained_cli_executions": 0,
            "roadmap_task_completion": False,
            "scope": "readback of fixed existing executions; original failures/unknown/not_run retained",
        }
        write_new(self.work / "platform-readback.json", result)
        return result


def review_platform(audit_root: Path, source: Path, controller: Path) -> dict:
    """Return all group results. Initialization refusal cannot certify any scope."""
    reader = None
    try:
        reader = Readback(audit_root, source, controller)
        return reader.run()
    except Exception as error:
        return {"schema": "a23-platform-readback-v1", "source_commit": CONFIG["source_commit"],
                "status": "failed_readback", "accepted": False,
                "groups": reader.groups if reader is not None else {},
                "error_type": type(error).__name__, "error": str(error), "waived": False,
                "new_native_measurements": 0, "new_product_or_fault_executions": 0,
                "new_retained_cli_executions": 0, "roadmap_task_completion": False}
