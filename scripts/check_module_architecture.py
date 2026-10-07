#!/usr/bin/env python3
"""Bounded source/doc drift checks, not a complete Rust or information-flow analyzer."""
from pathlib import Path
import json,re,sys
root=Path(__file__).resolve().parent.parent
caps=json.loads((root/'docs/internals/MODULE_CAPABILITIES.json').read_text())
def number(path,name):
    text=(root/path).read_text();m=re.search(r'pub(?:\(crate\))? const '+name+r': u32 = (\d+);',text)
    if not m:raise AssertionError('constant missing: '+name)
    return int(m[1])

def schema_contract(declaration,migration):
    def constant(name):
        match=re.search(r'pub(?:\(crate\))? const '+name+r': u32 = (\d+);',migration)
        assert match,'schema contract constant missing: '+name
        return int(match[1])
    # This is a declaration consistency check, not dynamic migration evidence.
    # Rust contract tests separately prove rebuild and generation behavior.
    assert type(declaration.get('database_schema')) is int, 'database_schema must be an integer'
    assert declaration['database_schema']==constant('CURRENT_SCHEMA_VERSION'), 'database_schema differs from production'
    compatibility=declaration.get('database_schema_compatibility')
    assert isinstance(compatibility,dict), 'database_schema_compatibility declaration missing'
    assert set(compatibility)=={'policy','contract_tests'}, 'schema compatibility fields missing or unsupported (including legacy additive assumptions)'
    assert compatibility['policy']=='rebuild_on_mismatch', 'unsupported schema compatibility policy'
    assert compatibility['contract_tests']==['crates/cc-db/tests/ci_schema_guard_contract.rs'], 'schema contract test reference differs from current Rust guard'

schema_contract(caps,(root/'crates/cc-db/src/index_migrate.rs').read_text())
compat=caps['database_schema_compatibility']
assert caps['project_model_version']==number('crates/cc-model/src/project_model.rs','PROJECT_MODEL_VERSION')
assert not (root/'crates/cc-parsers/src/import_resolver.rs').exists()
for directory in ['crates/cc-parsers/src','crates/cc-index/src']:
    for p in (root/directory).rglob('*.rs'):
        assert 'import_resolver::' not in p.read_text(), str(p)
for p in (root/'crates/cc-index/src/module_resolution').glob('*.rs'):
    # Deliberately narrow lexical guard. Fixture deletion and runtime tests are
    # separate evidence; this is not a proof against aliases or dynamic code.
    assert not re.search(r'std::fs::|std::process::|File::open\s*\(|Command::new\s*\(|reqwest::|TcpStream::',p.read_text()),str(p)
paths=[caps['module_owner'],caps['input_reader']]+caps['gate_tests']+compat['contract_tests']
for value in caps['languages'].values():paths.extend(value['tests'])
assert all((root/p).is_file() for p in paths)
for p in ['docs/LANGUAGES.md','docs/CONFIGURATION.md','docs/internals/INDEXING.md']:
    assert 'MODULE_INPUT_SAFETY.md' in (root/p).read_text(),p
print(json.dumps({'status':'passed','database_schema':caps['database_schema'],'database_schema_policy':compat['policy'],'project_model_version':caps['project_model_version'],'legacy_module_path_removed':True,'module_io_guard':'narrow lexical source guard; not complete static analysis','declared_test_files':sorted(set(paths))},indent=2))
