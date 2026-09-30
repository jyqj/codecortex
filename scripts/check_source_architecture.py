#!/usr/bin/env python3
"""Narrow source-evidence ownership checks; not a whole-program proof."""
from pathlib import Path
import json,re
root=Path(__file__).resolve().parent.parent
boundary=(root/'crates/cc-parsers/src/chunker/boundaries.rs').read_text()
assert 'Parser::new' not in boundary and '.parse(' not in boundary
assert 'SourceStructure' in boundary and 'tree.walk()' in boundary
for path in ['jsts/mod.rs','python/mod.rs','rust.rs','go.rs','java.rs','c_cpp.rs']:
    text=(root/'crates/cc-parsers/src'/path).read_text()
    assert 'chunk_with_tree(' in text and 'source_structure: Some(' in text,path
for name in ['index_db_write_batch.rs','index_db_multi_insert.rs']:
    text=(root/'crates/cc-db/src'/name).read_text()
    assert 'source_json' in text and 'c.source_json()' in text,name
text=(root/'crates/cc-parsers/src/chunker.rs').read_text().split('#[cfg(test)]')[0]
assert '.lines()' not in text and 'source.slice_digest(' in text
assert 'pub fn new(line_budget' not in text and 'pub fn from_policy(' in text
assert 'split::Partition::new' in text and 'merge::coalesce' in text
for crate in ['cc-model','cc-db','cc-index']:
    for source in (root/'crates'/crate/'src').rglob('*.rs'):
        assert 'tree_sitter::Tree' not in source.read_text(),source
assert 'SOURCE_CHUNKS.md' in (root/'docs/internals/INDEXING.md').read_text()
assert 'source_structure' in (root/'crates/cc-model/src/parse.rs').read_text()
registry=(root/'crates/cc-parsers/src/lib.rs').read_text()
assert 'with_chunk_policy' in registry and 'self.chunk_policy.validate()?' in registry
assert re.search(r'jsts\s*\.chunker', (root/'crates/cc-parsers/src/sfc.rs').read_text())
assert 'chunk_policy' in (root/'crates/cc-db/src/file_state_cache.rs').read_text()
for name in ['index_db_write_batch.rs','index_db_multi_insert.rs']:
    assert 'indexed_at,chunk_policy' in (root/'crates/cc-db/src'/name).read_text()
projection=(root/'crates/cc-index/src/documents/render.rs').read_text()
assert 'source.slice(proof.span)' in projection and 'text.push_str(&chunk.text)' in projection
assert 'std::fs::' not in projection and 'reqwest::' not in projection
assert (root/'docs/internals/CHUNK_POLICY.md').is_file()
for name in ['index_db_write_batch.rs','index_db_multi_insert.rs']:
    assert 'document_store::insert_on' in (root/'crates/cc-db/src'/name).read_text()
assert 'document_manifest' in (root/'crates/cc-db/src/sql/index_v1.sql').read_text()
for name in ['engine_query.rs','graph_trace.rs','handlers/context.rs']:
    assert 'read_verified' in (root/'crates/cc-server/src'/name).read_text()
engine=(root/'crates/cc-server/src/engine.rs').read_text()
assembly=engine[engine.index('fn assemble_context_once'):]
chain=['EvidenceHydrator::new','verifier.hydrate','coverage::select','budget::pack','verifier.finish']
positions=[assembly.index(step) for step in chain]
assert positions==sorted(positions),'final evidence must precede selection/packing and end with a read-view fence'
hydrator=(root/'crates/cc-search/src/evidence_hydrator.rs').read_text()
assert 'SourceVerifier::new' in hydrator and 'verify_source_records' in hydrator
assert 'read_generation()?' in hydrator and 'proof.validate(&hit.text)' in hydrator
assert re.search(r'scope\s*\.passes',hydrator)
assert 'validate_envelope_generation' in (root/'crates/cc-server/src/handlers/context.rs').read_text()
assert 'pub fn verify_source_records' in (root/'crates/cc-db/src/document_store.rs').read_text()
assert 'pub mod identity;' in (root/'crates/cc-model/src/lib.rs').read_text()
print(json.dumps({'status':'passed','scope':'lexical ownership/single-chunker/no-persisted-tree checks plus explicit runtime/unit tests; not full program verification','ast_producers':6,'production_chunk_core':'SourceStructure -> Partition -> coalesce','legacy_line_constructor_removed':True,'persisted_tree_types':0,'storage_paths':['scalar','table_major'],'source_doc':'docs/internals/SOURCE_CHUNKS.md'},indent=2))
