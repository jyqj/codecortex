"""Safe identity adaptation tests; no build or real product/100k launch."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
BASE = Path(__file__).resolve().parent
ORIGINAL = BASE.parents[2] / 'scripts/resource_harness'
PKG = BASE / 'harness/resource_harness'
spec = importlib.util.spec_from_file_location('qname_harness', PKG/'__init__.py', submodule_search_locations=[str(PKG)])
module = importlib.util.module_from_spec(spec)
import sys
sys.modules[spec.name] = module
spec.loader.exec_module(module)
from qname_harness import identity, driver

class CurrentIdentityTests(unittest.TestCase):
    def test_method_guard_criteria_and_historical_manifest_unchanged(self):
        for name in ('build.py','measurement.py','product.py','protocol.py','runtime.py','protocol-manifest.json','analyze_existing.py','__init__.py'):
            self.assertEqual((PKG/name).read_bytes(), (ORIGINAL/name).read_bytes(), name)
        self.assertEqual(identity.SOURCES, {'candidate':'90858afae647a513537bf118932a7ba5020ee98b'})
        self.assertIn('current-source.json', identity.driver_identity())
        self.assertEqual(json.loads((PKG/'current-source.json').read_text())['method_manifest_sha256'], identity.digest(PKG/'protocol-manifest.json'))

    def test_exact_identity_changes_only(self):
        old = (ORIGINAL/'identity.py').read_text()
        old = old.replace("SOURCES = dict(baseline='513a98c9a94b15ec77153df41af26fa3c8c0b5e8',\n               candidate='e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207')", "SOURCES = dict(candidate='90858afae647a513537bf118932a7ba5020ee98b')")
        old = old.replace("'protocol-manifest.json': digest(directory / 'protocol-manifest.json')}", "'protocol-manifest.json': digest(directory / 'protocol-manifest.json'),\n        'current-source.json': digest(directory / 'current-source.json')}")
        self.assertEqual(old, (PKG/'identity.py').read_text())
        old = (ORIGINAL/'driver.py').read_text().replace('Reusable PR126 driver.', 'Named qname-packing current-source PR126-method driver.').replace("    prepare_parser.add_argument('--baseline-root', required=True, type=Path)\n", '').replace('explicitly build BOTH versions; no product', 'explicitly build the fixed current version; no product').replace('prepare(dict(baseline=args.baseline_root.resolve(), candidate=args.candidate_root.resolve()),', 'prepare(dict(candidate=args.candidate_root.resolve()),')
        self.assertEqual(old, (PKG/'driver.py').read_text())

    def test_receipt_missing_or_changed_prevents_product(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with self.assertRaises(FileNotFoundError):
                driver.run_single(root/'missing','candidate',root/'out',root,'unknown',measure_fn=lambda *a,**k:self.fail('launch'))
            self.assertFalse((root/'out').exists())

    def test_own_smoke_then_exactly_one_formal_failure_preserved(self):
        for smoke_pass in (False, True):
            with self.subTest(smoke_pass=smoke_pass), tempfile.TemporaryDirectory() as temp:
                root = Path(temp);ready=root/'ready';ready.write_text('fixture')
                bundle={'receipts':{'candidate':{'sha256':'fixture'}}}
                receipts={'candidate':{'source_root':str(root)}}
                calls=[]
                def measure(*args, **kw):
                    calls.append(kw.get('preflight',False))
                    return dict(status='passed_preflight_32' if smoke_pass and kw else 'failed', files=32 if kw else 100000,failures=[] if smoke_pass and kw else ['retained failure'],not_run=[])
                with patch.object(driver,'load_bundle',return_value=(bundle,receipts)),patch.object(driver,'compiler_scan',return_value={}),patch.object(driver,'precheck',return_value={}):
                    if smoke_pass:self.assertEqual(driver.run_single(ready,'candidate',root/'out',root,'unknown',measure_fn=measure),1)
                    else:
                        with self.assertRaisesRegex(RuntimeError,'no 100k'):driver.run_single(ready,'candidate',root/'out',root,'unknown',measure_fn=measure)
                state=json.loads((root/'out/session.json').read_text())
                self.assertEqual(calls,[True,False] if smoke_pass else [True])
                self.assertEqual(state['formal_runs'],int(smoke_pass))
                self.assertEqual(state['status'],'failed')

if __name__ == '__main__': unittest.main(verbosity=2)
