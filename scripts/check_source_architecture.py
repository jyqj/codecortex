#!/usr/bin/env python3
"""Narrow source-evidence ownership checks; not a whole-program proof."""
from pathlib import Path
import json,re,subprocess,sys

def rust_tokens(source):
    """Small lexical reader: comments cannot satisfy a generation hook guard.

    Strings/chars stay opaque tokens so their braces do not alter body scope.
    This is a narrow structural guard, not a Rust control-flow analyzer.
    """
    tokens=[]; i=0
    while i<len(source):
        if source[i].isspace(): i+=1; continue
        if source.startswith('//',i):
            end=source.find('\n',i); i=len(source) if end<0 else end+1; continue
        if source.startswith('/*',i):
            depth=1; i+=2
            while depth:
                assert i<len(source),'unterminated block comment'
                if source.startswith('/*',i): depth+=1; i+=2
                elif source.startswith('*/',i): depth-=1; i+=2
                else: i+=1
            continue
        raw=re.match(r'r(#{0,8})"',source[i:])
        if raw:
            end=source.find('"'+raw[1],i+len(raw[0])); assert end>=0
            end+=1+len(raw[1]); tokens.append(source[i:end]); i=end; continue
        literal=re.match(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])\'',source[i:],re.S)
        word=re.match(r'\w+',source[i:])
        match=literal or word
        if match: tokens.append(match[0]); i+=len(match[0])
        else: tokens.append(source[i]); i+=1
    return tokens

def sequence(tokens,pattern):
    wanted=rust_tokens(pattern)
    for start in range(len(tokens)-len(wanted)+1):
        if tokens[start:start+len(wanted)]==wanted: return start
    raise AssertionError('generation contract missing: '+pattern)

def production_tokens(source):
    tokens=rust_tokens(source); marker=rust_tokens('#[cfg(test)]')
    while True:
        starts=[i for i in range(len(tokens)-len(marker)+1) if tokens[i:i+len(marker)]==marker]
        if not starts: return tokens
        start=starts[0]; body=tokens.index('{',start); depth=1
        for end in range(body+1,len(tokens)):
            depth+=int(tokens[end]=='{')-int(tokens[end]=='}')
            if depth==0: break
        assert depth==0,'unterminated test-only item'
        del tokens[start:end+1]

def function_body(tokens,name):
    start=sequence(tokens,'fn '+name)
    start=tokens.index('{',start); depth=1
    for end in range(start+1,len(tokens)):
        depth+=int(tokens[end]=='{')-int(tokens[end]=='}')
        if depth==0: return tokens[start+1:end]
    raise AssertionError('unterminated function: '+name)

def generation_contract(sources):
    engine=production_tokens(sources['engine'])
    owned=engine[sequence(engine,'impl crate::query_handle::QueryHandle'):]
    search=function_body(owned,'search_in_context_with')
    sequence(search,'.with_stable_generation(|generation| { self.assemble_context_once(query, top_k, intent, overrides.clone(), generation) })')
    assembly=function_body(owned,'assemble_context_once')
    order=[sequence(assembly,p) for p in ['EvidenceHydrator::new(', 'verifier.hydrate(', 'coverage::select_with_query(', 'budget::pack(envelope, max_output_chars)?', 'verifier.finish()?', 'Ok(envelope)']]
    assert order==sorted(order),'generation fence must cover the final packed envelope'
    sequence(assembly,'generation, control.clone()')
    sequence(assembly,'let source_freshness = verifier.diagnostics()')
    sequence(assembly,'"source_freshness": source_freshness')
    fence=function_body(production_tokens(sources['fence']),'with_stable_generation')
    sequence(fence,'let before = self.observe_epochs()?')
    sequence(fence,'let result = work(before)')
    sequence(fence,'let after = self.db.reads().read_generation()?')
    sequence(fence,'if before == after { return result.map(|value| (before, value)); }')
    sequence(fence,'Err(cc_model::CcError::RetrievalChanged { attempts: FENCE_MAX_ATTEMPTS, })')
    finish=function_body(production_tokens(sources['hydrator']),'finish')
    sequence(finish,'if self.db.reads().read_generation()? != self.generation { return Err(CcError::RetrievalChanged { attempts: 1 }); }')
    query=function_body(production_tokens(sources['query']),'search_async')
    sequence(query,'handle.search_in_context_with(&query, top_k, intent, overrides)')
    context=production_tokens(sources['context'])
    dispatch=function_body(context,'search_async')
    sequence(dispatch,'handle.search_async(query, top_k, intent, overrides).await?')
    sequence(dispatch,'finalize_search_response(&db, Some(before), value, max_bytes)')
    final=function_body(context,'finalize_search_response')
    sequence(final,'let accepted = super::freshness::accepted_generation_of(&value)')
    sequence(final,'super::freshness::attach_observed(before, db.reads().resolution_freshness()?, accepted, value,)?')
    sequence(final,'cc_search::selection::budget::pack_value(value, max_bytes)')
    freshness=function_body(production_tokens(sources['freshness']),'accepted_generation_of')
    sequence(freshness,'.pointer("/evidence_summary/source_freshness/generation")')

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
generation_contract({name:(root/path).read_text() for name,path in {
    'engine':'crates/cc-server/src/engine.rs',
    'fence':'crates/cc-search/src/engine_cache.rs',
    'hydrator':'crates/cc-search/src/evidence_hydrator.rs',
    'query':'crates/cc-server/src/query_handle.rs',
    'context':'crates/cc-server/src/handlers/context.rs',
    'freshness':'crates/cc-server/src/handlers/freshness.rs',
}.items()})
assert 'pub fn verify_source_records' in (root/'crates/cc-db/src/document_store.rs').read_text()
assert 'pub mod identity;' in (root/'crates/cc-model/src/lib.rs').read_text()
print(json.dumps({'status':'passed','scope':'lexical ownership/single-chunker/no-persisted-tree checks plus explicit runtime/unit tests; not full program verification','generation_guard':'function-scoped assembly callback, hydrator finish, persistent-generation comparison and accepted-generation dispatch annotation; comments excluded','generation_behavior_tests':'crates/cc-eval/tests/ci_generation_guard_contract.rs','ast_producers':6,'production_chunk_core':'SourceStructure -> Partition -> coalesce','legacy_line_constructor_removed':True,'persisted_tree_types':0,'storage_paths':['scalar','table_major'],'source_doc':'docs/internals/SOURCE_CHUNKS.md'},indent=2))
if __name__=='__main__':
    subprocess.run([sys.executable,str(root/'scripts/tests/test_p7_ci_architecture_guards.py')],check=True)
