#!/usr/bin/env python3
"""Mechanical read-only check of the reviewed duplicate-test removal."""
from pathlib import Path
import hashlib
import json
import re
import textwrap

D = Path(__file__).resolve().parent
OLD = (D / 'fd9-inputs/crates/cc-search/src/engine.rs').read_text()
NEW = (D / 'main55-inputs/crates/cc-search/src/engine.rs').read_text()
OWNER = (D / 'main55-inputs/crates/cc-search/src/engine_lane_tests.rs').read_text()


def masked(text):
    """Mask comments and Rust string/char literals while retaining offsets."""
    chars = list(text)
    i = 0
    while i < len(text):
        end = None
        if text.startswith('//', i):
            end = text.find('\n', i)
            if end < 0:
                end = len(text)
        elif text.startswith('/*', i):
            depth = 1
            end = i + 2
            while depth:
                assert end < len(text)
                if text.startswith('/*', end):
                    depth += 1
                    end += 2
                elif text.startswith('*/', end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
        else:
            raw = re.match(r'(?:br|r)(#*)"', text[i:])
            if raw:
                close = '"' + raw.group(1)
                close_at = text.find(close, i + len(raw.group(0)))
                assert close_at >= 0
                end = close_at + len(close)
            elif text[i] == '"':
                end = i + 1
                while True:
                    assert end < len(text)
                    if text[end] == '\\':
                        end += 2
                    elif text[end] == '"':
                        end += 1
                        break
                    else:
                        end += 1
            elif text[i] == "'":
                char = re.match(r"'(?:\\(?:u\{[0-9A-Fa-f_]+\}|x[0-9A-Fa-f]{2}|[^\n])|[^\\'\n])'", text[i:])
                if char:
                    end = i + len(char.group(0))
        if end is not None:
            for n in range(i, end):
                if chars[n] != '\n':
                    chars[n] = ' '
            i = end
        else:
            i += 1
    return ''.join(chars)


def functions(text, indent):
    mask = masked(text)
    result = {}
    for start in re.finditer(r'(?m)^' + ' ' * indent + r'fn ([A-Za-z0-9_]+)\(', mask):
        pos = mask.index('{', start.end())
        depth = 1
        end = pos + 1
        while depth:
            assert end < len(mask)
            depth += (mask[end] == '{') - (mask[end] == '}')
            end += 1
        assert start.group(1) not in result
        result[start.group(1)] = textwrap.dedent(text[start.start():end])
    return result


marker = '#[cfg(test)]\nmod tests'
old = functions(OLD.split(marker, 1)[1], 4)
new = functions(NEW.split(marker, 1)[1], 4)
owner = functions(OWNER, 0)
removed = sorted(set(old) - set(new))
old_tests = set(re.findall(r'(?m)^    #\[test\]\n    fn ([A-Za-z0-9_]+)\(', masked(OLD)))
new_tests = set(re.findall(r'(?m)^    #\[test\]\n    fn ([A-Za-z0-9_]+)\(', masked(NEW)))
remaining = sorted(new_tests)
assert set(new) <= set(old)
assert all(new[name] == old[name] for name in new)
assert len(new) == 21 and len(old) == 40  # Four other helper functions are retained.
assert len(old_tests) == 35 and len(new_tests) == 17
assert old_tests - new_tests == set(removed) - {'fake_candidate_chunk'}
assert len(removed) == 19 and 'fake_candidate_chunk' in removed
assert all(name in owner for name in removed)
rows = []
for name in removed:
    before = old[name]
    retained = owner[name]
    equal = before == retained
    exception = None
    if not equal:
        assert name == 'lexical_lane_adapter_matches_inline_ranking'
        old_comment = ('    // P5-A preserves native BM25 in lane diagnostics; the legacy rank\n'
                       '    // projection and candidate order remain unchanged.\n')
        new_comment = '    // Native diagnostic score is now separate from the legacy rank slot.\n'
        assert before.count(old_comment) == 1
        assert before.replace(old_comment, new_comment) == retained
        exception = 'Only the exact two-line versus one-line diagnostic-score comment differs; every executable byte is unchanged.'
    rows.append({'name': name, 'is_test': name != 'fake_candidate_chunk',
                 'retained_path': 'crates/cc-search/src/engine_lane_tests.rs',
                 'equal_after_module_indentation': equal, 'exception': exception,
                 'original_function_sha256': hashlib.sha256(before.encode()).hexdigest(),
                 'retained_function_sha256': hashlib.sha256(retained.encode()).hexdigest()})
old_production = OLD.split(marker, 1)[0]
new_production = NEW.split(marker, 1)[0]
assert old_production == new_production
assert '[cfg(test)]\nmod engine_lane_tests;' in (D / 'main55-inputs/crates/cc-search/src/lib.rs').read_text()
result = {'schema': 'main55-engine-static-review-v1', 'reviewer': '/root/pr_audit',
          'comparison': ['fd9ca5db6485b575872c722995c451db926d05c9', '55aa2bcf355441585bcf980e1d6f4fab8eebe59d'],
          'production_bytes_equal': True, 'production_bytes': len(new_production.encode()),
          'production_sha256': hashlib.sha256(new_production.encode()).hexdigest(),
          'tests_before': 35, 'tests_after': 17, 'removed_duplicate_tests': 18,
          'removed_duplicate_helpers': 1, 'remaining_test_bodies_exact': remaining,
          'retained_helper_bodies_exact': sorted(set(new) - new_tests),
          'removed_bodies': rows, 'owner_still_registered_under_cfg_test': True,
          'product_compilation_or_tests_run': False,
          'limit': 'Mechanical source review of existing fixed bytes. No original TODO closure or new runtime result.'}
(D / 'engine-static-review.json').write_text(json.dumps(result, sort_keys=True, indent=2) + '\n')
print(json.dumps({'production_bytes_equal': True, 'removed_duplicate_tests': 18,
                  'removed_duplicate_helpers': 1, 'remaining_tests_exact': 17,
                  'comment_only_exceptions': sum(row['exception'] is not None for row in rows)}))
