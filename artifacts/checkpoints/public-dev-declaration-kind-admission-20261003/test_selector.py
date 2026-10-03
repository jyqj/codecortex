#!/usr/bin/env python3
"""Small adversarial fixtures plus fixed-byte loader integration; no scoring."""
import copy
import json
import unittest
from unittest.mock import patch
import selector as s


class SelectorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest, cls.originals = s.checked_inputs()
        cls.sources = {k: v for k, v in cls.originals.items() if '/source/' in k}

    def load(self, **kw):
        args = dict(version=s.VERSION, repositories=['requests', 'gin'], source_bytes=self.sources)
        args.update(kw)
        return s.load_version(**args)

    def test_exact_integration_and_compat(self):
        packages = self.load()
        self.assertEqual(sum(len(p['native'].splitlines()) for p in packages.values()), 158)
        self.assertEqual(sum(len(p['compat'].splitlines()) for p in packages.values()), 138)
        for repo, package in packages.items():
            self.assertEqual(package['compat'], self.originals[package['input_lock']['compat_entry']])
            self.assertEqual(s.sha(package['native']), self.manifest['files'][repo]['after_raw_sha256'])

    def test_missing_or_wrong_version(self):
        for version in (None, '', 'default', 'public-dev-declaration-kind-v2-candidate'):
            with self.assertRaisesRegex(ValueError, 'explicit admitted version'):
                self.load(version=version)
        with self.assertRaises(TypeError):
            s.load_version(repositories=s.REPOS, source_bytes=self.sources)

    def test_repository_subsets_extras_duplicates(self):
        for repos in (['requests'], ['gin'], ['requests', 'requests'],
                      ['requests', 'gin', 'express'], ['typescript', 'gin']):
            with self.assertRaisesRegex(ValueError, 'repository scope'):
                self.load(repositories=repos)

    def test_source_byte_mutation(self):
        actual = dict(self.sources)
        entry = next(iter(actual))
        actual[entry] += b'\n'
        with self.assertRaisesRegex(ValueError, 'source bytes'):
            s.validate_sources(actual, self.originals)

    def test_source_missing_extra_wrong_author(self):
        entry = next(iter(self.sources))
        for variant in ('missing', 'extra', 'wrong_author'):
            actual = dict(self.sources)
            if variant == 'missing':
                del actual[entry]
            elif variant == 'extra':
                actual['fake:crates/cc-eval/benchmarks/public-v19/express/source/x.js'] = b''
            else:
                actual['0' * 40 + ':' + entry.split(':', 1)[1]] = actual.pop(entry)
            with self.assertRaisesRegex(ValueError, 'source set'):
                s.validate_sources(actual, self.originals)

    def test_caller_digest_is_not_source_bytes(self):
        actual = {k: s.sha(v) for k, v in self.sources.items()}
        with self.assertRaisesRegex(ValueError, 'source bytes'):
            s.validate_sources(actual, self.originals)

    def test_original_external_pin_rejected(self):
        original_blob = s.blob
        entry = self.manifest['files']['requests']['entry']
        def wrong_blob(key):
            raw = original_blob(key)
            return raw + b' ' if key == entry else raw
        with patch.object(s, 'blob', wrong_blob):
            with self.assertRaisesRegex(ValueError, 'original input pin'):
                s.checked_inputs()

    def test_self_resealed_selector_still_rejected(self):
        read = s.Path.read_bytes
        def wrong_read(path):
            raw = read(path)
            if path == s.HERE / 'selector.json':
                obj = json.loads(raw)
                obj['inputs']['requests']['native_after_sha256'] = '0' * 64
                return json.dumps(obj).encode()
            return raw
        with patch.object(s.Path, 'read_bytes', wrong_read):
            with self.assertRaisesRegex(ValueError, 'selector pin'):
                self.load()

    def test_receipt_authority_mutation_rejected(self):
        read = s.Path.read_bytes
        def wrong_read(path):
            raw = read(path)
            return raw + b' ' if path == s.HERE / 'admission-receipt.json' else raw
        with patch.object(s.Path, 'read_bytes', wrong_read):
            with self.assertRaisesRegex(ValueError, 'receipt pin'):
                self.load()

    def patch_inputs(self):
        info = self.manifest['files']['requests']
        return (self.originals[info['entry']],
                [copy.deepcopy(c) for c in self.manifest['changes'] if c['binding']['repo'] == 'requests'],
                info['after_raw_sha256'])

    def test_subset_extra_duplicate_delta_rejected(self):
        original, changes, digest = self.patch_inputs()
        with self.assertRaisesRegex(ValueError, 'derived external pin'):
            s.apply_native(original, changes[:-1], digest)
        with self.assertRaisesRegex(ValueError, 'duplicate location'):
            s.apply_native(original, changes + [changes[0]], digest)

    def test_wrong_pointer_byte_offset_and_alias(self):
        for mutation in ('pointer', 'offset', 'alias'):
            original, changes, digest = self.patch_inputs()
            d = changes[0]['delta']
            if mutation == 'pointer':
                d['pointer'] += '/dtype'
            elif mutation == 'offset':
                d['file_byte_start'] += 1
            else:
                d['after_raw_token'] = '"*"'
            with self.assertRaises(ValueError):
                s.apply_native(original, changes, digest)

    def test_non_kind_source_binding_mutation(self):
        original, changes, digest = self.patch_inputs()
        changes[0]['source_binding_after']['source_sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'source binding changed'):
            s.apply_native(original, changes, digest)


if __name__ == '__main__':
    unittest.main(verbosity=2)
