"""Negative mutations of production-scoped generation checks; no source writes."""
from pathlib import Path
import contextlib
import io
import copy
import json
import runpy
import unittest

ROOT = Path(__file__).resolve().parents[2]
with contextlib.redirect_stdout(io.StringIO()):
    GUARD = runpy.run_path(str(ROOT / 'scripts/check_source_architecture.py'))
    MODULE = runpy.run_path(str(ROOT / 'scripts/check_module_architecture.py'))
PATHS = {
    'engine': 'crates/cc-server/src/engine.rs',
    'fence': 'crates/cc-search/src/engine_cache.rs',
    'hydrator': 'crates/cc-search/src/evidence_hydrator.rs',
    'query': 'crates/cc-server/src/query_handle.rs',
    'context': 'crates/cc-server/src/handlers/context.rs',
    'freshness': 'crates/cc-server/src/handlers/freshness.rs',
}

class GenerationGuardMutations(unittest.TestCase):
    def setUp(self):
        self.sources = {name: (ROOT / path).read_text() for name, path in PATHS.items()}

    def test_current_production_chain_passes(self):
        GUARD['generation_contract'](self.sources)

    def test_removed_or_changed_real_boundaries_fail(self):
        cases = [
            ('engine', '.with_stable_generation(|generation| {', '.without_stable_generation(|generation| {'),
            ('engine', 'verifier.finish()?;', '// verifier.finish()?;'),
            ('fence', 'if before == after {', 'if before != after {'),
            ('hydrator', 'self.db.reads().read_generation()? != self.generation', 'self.db.reads().read_generation()? == self.generation'),
            ('context', 'finalize_search_response(&db, Some(before), value, max_bytes)', 'unfenced_response(&db, Some(before), value, max_bytes)'),
            ('freshness', '"/evidence_summary/source_freshness/generation"', '"/unrelated/generation"'),
        ]
        for owner, old, new in cases:
            with self.subTest(owner=owner, mutation=old):
                sources = self.sources.copy()
                self.assertIn(old, sources[owner])
                sources[owner] = sources[owner].replace(old, new)
                with self.assertRaises(AssertionError):
                    GUARD['generation_contract'](sources)

    def test_comment_or_test_module_cannot_supply_missing_production_finish(self):
        sources = self.sources.copy()
        sources['engine'] = sources['engine'].replace('verifier.finish()?;', '// verifier.finish()?;')
        sources['engine'] += '\n#[cfg(test)] mod decoy { fn fake() { verifier.finish()?; } }\n'
        with self.assertRaises(AssertionError):
            GUARD['generation_contract'](sources)

    def test_same_named_test_only_function_cannot_replace_real_hydrator_finish(self):
        sources = self.sources.copy()
        source = sources['hydrator']
        start = source.index('    pub fn finish(&self)')
        end = source.index('    pub fn diagnostics', start)
        removed = source[start:end]
        sources['hydrator'] = source[:start] + source[end:] + '\n#[cfg(test)] mod decoy {\n' + removed + '\n}\n'
        with self.assertRaises(AssertionError):
            GUARD['generation_contract'](sources)

class SchemaDeclarationMutations(unittest.TestCase):
    def test_current_contract_and_wrong_schema_or_predecessor(self):
        current = json.loads((ROOT / 'docs/internals/MODULE_CAPABILITIES.json').read_text())
        migration = (ROOT / 'crates/cc-db/src/index_migrate.rs').read_text()
        MODULE['schema_contract'](current, migration)
        old = copy.deepcopy(current)
        old['database_schema'] = 21
        with self.assertRaises(AssertionError):
            MODULE['schema_contract'](old, migration)
        wrong_predecessor = copy.deepcopy(current)
        wrong_predecessor['database_schema_compatibility']['additive_migration_from'] = 20
        with self.assertRaises(AssertionError):
            MODULE['schema_contract'](wrong_predecessor, migration)

if __name__ == '__main__':
    unittest.main()
