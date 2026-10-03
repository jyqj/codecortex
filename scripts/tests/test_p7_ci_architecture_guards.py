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
    def setUp(self):
        self.current = json.loads((ROOT / 'docs/internals/MODULE_CAPABILITIES.json').read_text())
        self.migration = (ROOT / 'crates/cc-db/src/index_migrate.rs').read_text()

    def test_current_contract(self):
        MODULE['schema_contract'](self.current, self.migration)

    def test_wrong_or_missing_schema_fails(self):
        current = self.current['database_schema']
        for schema in [21, 22, 23, current - 1, current + 1, str(current), None, True]:
            with self.subTest(schema=schema):
                declaration = copy.deepcopy(self.current)
                declaration['database_schema'] = schema
                with self.assertRaises(AssertionError):
                    MODULE['schema_contract'](declaration, self.migration)
        del self.current['database_schema']
        with self.assertRaises(AssertionError):
            MODULE['schema_contract'](self.current, self.migration)

    def test_wrong_missing_or_contradictory_policy_fails(self):
        current = self.current['database_schema_compatibility']
        cases = [None, {}, {'contract_tests': current['contract_tests']}]
        for policy in ['adjacent_additive', 'unknown', None]:
            cases.append({**current, 'policy': policy})
        for predecessor in [21, 23, None]:
            cases.append({**current, 'additive_migration_from': predecessor})
        cases.append({'additive_migration_from': 21, 'contract_tests': current['contract_tests']})
        for compatibility in cases:
            with self.subTest(compatibility=compatibility):
                declaration = copy.deepcopy(self.current)
                declaration['database_schema_compatibility'] = compatibility
                with self.assertRaises(AssertionError):
                    MODULE['schema_contract'](declaration, self.migration)
        del self.current['database_schema_compatibility']
        with self.assertRaises(AssertionError):
            MODULE['schema_contract'](self.current, self.migration)

    def test_missing_or_stale_rust_guard_reference_fails(self):
        for tests in [[], ['crates/cc-db/tests/missing.rs'], None]:
            with self.subTest(tests=tests):
                declaration = copy.deepcopy(self.current)
                declaration['database_schema_compatibility']['contract_tests'] = tests
                with self.assertRaises(AssertionError):
                    MODULE['schema_contract'](declaration, self.migration)
        del self.current['database_schema_compatibility']['contract_tests']
        with self.assertRaises(AssertionError):
            MODULE['schema_contract'](self.current, self.migration)

    def test_missing_production_version_fails(self):
        migration = self.migration.replace('const CURRENT_SCHEMA_VERSION:', 'const REMOVED_SCHEMA_VERSION:')
        with self.assertRaises(AssertionError):
            MODULE['schema_contract'](self.current, migration)

if __name__ == '__main__':
    unittest.main()
