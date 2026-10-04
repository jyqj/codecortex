#!/usr/bin/env python3
"""Explicit pinned PYGO DEV loader; no evaluator, scorer, or network calls."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
CANDIDATE = '97c478cdb05d2bb85f852ff42af5452b00ba6e3a'
INDEPENDENT = '47706907868007f71246f74674164263019bba79'
CP = 'artifacts/checkpoints/public-dev-declaration-kind-candidate-20261003/'
IP = 'artifacts/checkpoints/public-dev-pygo-v2-independent-20261003/'
VERSION = 'public-dev-pygo-declaration-kind-v2'
REPOS = ('requests', 'gin')
MANIFEST_PIN = 'f70f9b2ea562cb7fe8dca44f341e24671af0f84a072ff878e80f10fb67189507'
ARTIFACT_PIN = '1a0b9ed8a02eb770672d1f0baa7ef330451d865a6d174ea8eab4c8dc823603e9'
RECEIPT_PIN = 'df42891af6685435692737e47e95dc76af9e158022d10befedfb3d3a44cd19c9'
SELECTOR_PIN = '7c0250179dc76138168a1ee8ad95889d0afab8c632a4ca763f09bc2f43fd7700'


def require(ok, label):
    if not ok:
        raise ValueError(label)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def blob(entry):
    return subprocess.check_output(['git', 'show', entry], cwd=ROOT,
                                   stderr=subprocess.DEVNULL)


def fixed_artifacts(commit, prefix, expected_pin=None):
    raw = blob(commit + ':' + prefix + 'artifact-manifest.json')
    if expected_pin:
        require(sha(raw) == expected_pin, 'fixed artifact manifest pin')
    manifest = json.loads(raw)
    for path, pin in manifest['files'].items():
        content = blob(commit + ':' + prefix + path)
        require(sha(content) == pin['sha256'] and len(content) == pin['bytes'],
                'fixed artifact digest')
        require((ROOT / prefix / path).read_bytes() == content,
                'fixed worktree artifact drift')
    require((ROOT / prefix / 'artifact-manifest.json').read_bytes() == raw,
            'fixed worktree manifest drift')


def checked_inputs():
    fixed_artifacts(CANDIDATE, CP, ARTIFACT_PIN)
    fixed_artifacts(INDEPENDENT, IP)
    raw = blob(CANDIDATE + ':' + CP + 'change-manifest.json')
    require(sha(raw) == MANIFEST_PIN, 'fixed change manifest pin')
    manifest = json.loads(raw)
    pins = json.loads(blob(CANDIDATE + ':' + CP + 'input-manifest.json'))
    independent_pins = json.loads(blob(INDEPENDENT + ':' + IP + 'input-pins.json'))
    require(pins == independent_pins, 'independent input pins disagree')
    originals = {}
    for entry, digest in pins.items():
        originals[entry] = blob(entry)
        require(sha(originals[entry]) == digest, 'original input pin')
    # Entire original suite inventory, including files without reviewed alternatives.
    # Authority is the externally fixed suite blob/author SHA, not a fresh digest claim.
    for repo in REPOS:
        suite_entry = next(k for k in pins if f'/{repo}/suite-native-dev.json' in k)
        suite = json.loads(originals[suite_entry])
        base = suite_entry.rsplit('/', 1)[0] + '/' + suite['source']['root'] + '/'
        for path in suite['source']['files']:
            entry = base + path
            require('..' not in Path(path).parts and not Path(path).is_absolute(),
                    'unsafe source path')
            raw = blob(entry)
            if entry in originals:
                require(originals[entry] == raw, 'review/suite source disagreement')
            originals[entry] = raw
    legal = json.loads(blob(CANDIDATE + ':' + CP + 'license-retention-manifest.json'))
    for path, pin in legal.items():
        originals[pin['entry']] = blob(pin['entry'])
        require(sha(originals[pin['entry']]) == pin['sha256'], 'original legal pin')
        require(originals[pin['entry']] == blob(CANDIDATE + ':' + CP + path),
                'retained legal bytes')
    return manifest, originals


def source_inputs():
    """Read actual pinned Git source bytes; never accept caller-reported digests."""
    _, originals = checked_inputs()
    return {k: v for k, v in originals.items() if '/source/' in k}


def validate_sources(actual, originals):
    expected = {k: v for k, v in originals.items() if '/source/' in k}
    require(set(actual) == set(expected), 'source set or repository mix')
    require(all(isinstance(actual[k], bytes) and actual[k] == expected[k]
                for k in expected), 'source bytes or author SHA drift')


def apply_native(original, changes, expected_sha):
    lines = original.splitlines(keepends=True)
    offsets, offset = [], 0
    for line in lines:
        offsets.append(offset)
        offset += len(line)
    locations = set()
    patches = []
    for change in changes:
        binding, delta = change['binding'], change['delta']
        qi = binding['query_ordinal']
        gi, ai = binding['group_ordinal'], binding['alternative_ordinal']
        location = (qi, gi, ai)
        require(location not in locations, 'duplicate location')
        locations.add(location)
        line = lines[qi]
        require(sha(line) == binding['original_query_line_raw_sha256'], 'row binding')
        require(delta['pointer'] == f'/answers/{gi}/alternatives/{ai}/symbol/kind',
                'kind pointer')
        require(json.loads(line)['answers'][gi]['alternatives'][ai]['symbol']['kind']
                == 'function', 'original kind')
        require(change['source_binding_before'] == change['source_binding_after'],
                'source binding changed')
        start, end = delta['file_byte_start'], delta['file_byte_end']
        require((start, end) == (offsets[qi] + delta['line_byte_start'],
                                offsets[qi] + delta['line_byte_end']), 'byte location')
        require(delta['before_raw_token'] == '"function"' and
                delta['after_raw_token'] == '"method"' and
                original[start:end] == b'"function"', 'literal kind token')
        patches.append((start, end))
    patches.sort()
    require(all(a[1] <= b[0] for a, b in zip(patches, patches[1:])), 'overlap')
    derived = original
    for start, end in reversed(patches):
        derived = derived[:start] + b'"method"' + derived[end:]
    require(sha(derived) == expected_sha, 'derived external pin')
    restored = derived
    for i in range(len(patches) - 1, -1, -1):
        start = patches[i][0] - 2 * i
        require(restored[start:start + 8] == b'"method"', 'reverse placement')
        restored = restored[:start] + b'"function"' + restored[start + 8:]
    require(restored == original, 'non-kind byte changes')
    return derived


def load_version(*, version, repositories, source_bytes):
    """Return pinned inputs in memory. Both paired runners must call this API.

    source_bytes is an exact entry->bytes map from the actual runner corpus.
    No implicit/default version and no subset or additional repository accepted.
    """
    require(version == VERSION, 'explicit admitted version required')
    require(len(repositories) == 2 and set(repositories) == set(REPOS),
            'exact PYGO repository scope required')
    manifest, originals = checked_inputs()
    validate_sources(source_bytes, originals)
    receipt_raw = (HERE / 'admission-receipt.json').read_bytes()
    selector_raw = (HERE / 'selector.json').read_bytes()
    require(sha(receipt_raw) == RECEIPT_PIN, 'admission receipt pin')
    require(sha(selector_raw) == SELECTOR_PIN, 'selector pin')
    seal = json.loads((HERE / 'artifact-manifest.json').read_bytes())
    actual_paths = {str(p.relative_to(HERE)) for p in HERE.rglob('*') if p.is_file()
                    and p.relative_to(HERE).parts[0] not in ('.scratch', '__pycache__')
                    and p.name != 'artifact-manifest.json'}
    require(actual_paths == set(seal['files']), 'admission artifact set')
    for path, pin in seal['files'].items():
        raw = (HERE / path).read_bytes()
        require(sha(raw) == pin['sha256'] and len(raw) == pin['bytes'], 'admission seal')
    receipt, selector = json.loads(receipt_raw), json.loads(selector_raw)
    require(receipt['version'] == selector['version'] == VERSION and
            receipt['state'] == 'admitted_public_dev_opt_in', 'admission version/state')
    require(receipt['candidate_commit'] == CANDIDATE and
            receipt['independent_commit'] == INDEPENDENT and
            receipt['change_manifest_sha256'] == MANIFEST_PIN and
            receipt['candidate_artifact_manifest_sha256'] == ARTIFACT_PIN,
            'admission fixed provenance')
    require(selector['repositories'] == list(REPOS) and selector['default'] is False,
            'selector scope/default')
    require(sha((HERE / 'root-decision.txt').read_bytes()) == receipt['root_decision_sha256'],
            'root decision pin')
    require(selector['inputs'] == receipt['inputs'], 'selector receipt input mismatch')
    require(sha(blob(INDEPENDENT + ':' + IP + 'artifact-manifest.json')) ==
            receipt['independent_artifact_manifest_sha256'], 'independent manifest pin')
    admission_entry = next(k for k in originals if k.startswith(
        '5385f5a7a2a875c6d5cbd049bdde039bf71bbf32:') and k.endswith('/admission.json'))
    require(sha(originals[admission_entry]) == receipt['original_admission_sha256'] ==
            manifest['previous_admission_sha256'], 'original admission pin')
    require(receipt['source_bytes_sha256'] ==
            {k: sha(v) for k, v in originals.items() if '/source/' in k},
            'full original suite source pins')
    require(len(manifest['changes']) == 166, 'delta count')
    packages = {}
    for repo in REPOS:
        lock = selector['inputs'][repo]
        info = manifest['files'][repo]
        require(lock['native_entry'] == info['entry'] and
                lock['native_before_sha256'] == info['before_raw_sha256'] and
                lock['native_after_sha256'] == info['after_raw_sha256'], 'native selector pin')
        original = originals[lock['native_entry']]
        native = apply_native(original,
                              [c for c in manifest['changes'] if c['binding']['repo'] == repo],
                              lock['native_after_sha256'])
        compat = originals[lock['compat_entry']]
        require(sha(compat) == lock['compat_sha256'], 'compat selector pin')
        require(len(native.splitlines()) == lock['native_rows'] and
                len(compat.splitlines()) == lock['compat_rows'], 'row counts')
        suites = {mode: originals[lock[mode + '_suite_entry']]
                  for mode in ('native', 'compat')}
        require(all(sha(suites[mode]) == lock[mode + '_suite_sha256']
                    for mode in suites), 'original suite pin')
        packages[repo] = dict(native=native, compat=compat, suites=suites,
                              source_bytes={k: v for k, v in source_bytes.items()
                                            if f'/public-v19/{repo}/source/' in k},
                              input_lock=lock)
    return packages


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--version', required=True)
    parser.add_argument('--repositories', nargs='+', required=True)
    args = parser.parse_args()
    packages = load_version(version=args.version, repositories=args.repositories,
                            source_bytes=source_inputs())
    print(json.dumps(dict(version=VERSION, repositories=list(REPOS), native_rows=158,
                          compat_rows=138, kind_deltas=166, non_kind_changes=0,
                          independent_samples_added=0,
                          native_sha256={r: sha(p['native']) for r, p in packages.items()},
                          retrieval_runs=0, scores=0), sort_keys=True))


if __name__ == '__main__':
    main()
