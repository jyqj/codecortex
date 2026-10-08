function auditOriginal(readText, sha256, utf8) {
const need=(c,m)=>{if(!c)throw Error(m);};
const js=p=>JSON.parse(readText(p)), rows=p=>readText(p).trim().split("\n").map(JSON.parse);
const stable=x=>Array.isArray(x)?x.map(stable):x&&typeof x==="object"?Object.fromEntries(Object.keys(x).sort().map(k=>[k,stable(x[k])])):x;
const same=(a,b)=>JSON.stringify(stable(a))===JSON.stringify(stable(b));
const packageManifest=js("raw/candidate-package/manifest.json");
need(packageManifest.product_source==="2cd04485b0e0b23483f64671d56538bbaa47f442" && packageManifest.evidence_source===packageManifest.product_source,"package source");
const index=js("raw/external/external-reference-package/index.json"), original=new Map(index.original_files.map(r=>[r.path,r]));
need(original.size===1290 && index.source===packageManifest.product_source && index.third_party_query_or_gold_corpus_included===false,"original index identity");
const roundtrip=js("raw/external/external-reference-package/roundtrip.json");
need(roundtrip.all_files_restored_identically===true && roundtrip.files===1290 && roundtrip.total_bytes===49724321 && roundtrip.index_sha256==="bd8c836716aaf6e3fdf0df8945467d59480b1bbc5e33b6bf1f1ce4396f8851b8","actual original roundtrip receipt");
const output=[];
for(const repo of ["cc-switch","flask"]){
 const source=js("raw/external/sources/"+repo+".json"), lock=js("raw/external/locks/"+repo+".json"), receipt=js("derived-original/"+repo+"/receipt.json");
 const expected=packageManifest.datasets.find(d=>d.name===repo);
 need(lock.commit===expected.commit && lock.repository===expected.repository && lock.timeout_seconds===600,"dataset and command budget");
 need(source.common.length===expected.common_files && source.tracked.length===expected.all_tracked_files,"common source count");
 need(source.common.reduce((a,b)=>a+b.bytes,0)===expected.common_bytes && source.tracked.reduce((a,b)=>a+b.bytes,0)===expected.all_tracked_bytes,"source byte count");
 const common=new Map(source.common.map(f=>[f.path,f]));
 need(common.size===source.common.length,"unique source paths");
 need(receipt.exit_code===1 && receipt.status==="gate_not_passed" && receipt.release_certified===false,"quality failure retained");
 need(same(receipt.commands.map(c=>c.exit_code),[0,1,1,0,1,1]) && same(receipt.commands.map(c=>c.process_exit_code),[0,1,1,0,1,1]),"actual validate/run/replay exits");
 for(const profile of ["compat","native"]){
  const dir=repo+"/"+profile+"/",pre="derived-original/"+dir;
  const normalized=rows(pre+"normalized.jsonl"), scores=rows(pre+"scores.jsonl"), metrics=js(pre+"metrics.json"),gate=js(pre+"gate.json"),manifest=js(pre+"manifest.json"),suite=js("raw/external/suites/"+repo+"/suite."+profile+".json");
  const lockSuite=lock.suites.find(s=>s.profile===profile),observed=receipt.profiles[profile],identity=receipt.identity.suites[profile];
  need(same(suite,manifest.suite),"actual suite bytes semantic binding");
  need(identity.suite_sha256===lockSuite.sha256 && identity.query_sha256===lockSuite.query_sha256,"original suite/query exact hash lock; public suite is canonical JSON copy");
  need(same(identity.budget,packageManifest.budget)&&same(suite.engine_config,packageManifest.engine_config),"fixed budget/config");
  need(same({repetitions:suite.repetitions,seed:suite.seed,timeout_ms:suite.timeout_ms,top_k:suite.top_k,warmup:suite.warmup},packageManifest.budget),"actual budget");
  need(manifest.adapter==="mcp-stdio" && manifest.measurement_profile==="smoke" && manifest.input.commit===expected.commit,"actual adapter/profile/source");
  need(manifest.input.source_digest===suite.source.digest && manifest.input.query_digest===suite.queries_digest,"source/query digest");
  need(manifest.input.files.length===common.size && Object.keys(lockSuite.source_files).length===common.size,"locked source set size");
  const input=new Map(manifest.input.files.map(f=>[f.path,f]));
  for(const [p,f]of common){need(lockSuite.source_files[p]===f.sha256 && input.get(p)?.bytes===f.bytes,"source path/hash/length bound");}
  need(normalized.length===300 && scores.length===300 && metrics.queries===100 && metrics.measured_rows===300 && metrics.cases.length===100,"all rows denominator");
  const wanted=new Set(Array.from({length:100},(_,i)=>"Q"+String(i+1).padStart(2,"0")));
  need(new Set(metrics.cases.map(c=>c.id)).size===100 && metrics.cases.every(c=>wanted.has(c.id)&&c.repetitions===3),"case denominator");
  const pairs=new Set(),rawPaths=new Set(),statuses={};let hits=0;
  for(let n=0;n<normalized.length;n++){
   const row=normalized[n];need(wanted.has(row.case_id)&&Number.isInteger(row.repetition)&&row.repetition>=0&&row.repetition<3,"request membership");
   const pair=row.case_id+":"+row.repetition;need(!pairs.has(pair),"duplicate request");pairs.add(pair);
   need(/^raw\/[0-9]{6}\.json$/.test(row.raw_path)&&!rawPaths.has(row.raw_path),"unique raw response");rawPaths.add(row.raw_path);
   const oi=original.get(dir+row.raw_path),ri=observed.raw_files[row.raw_path];
   need(oi&&ri&&oi.bytes===ri.bytes&&oi.sha256===ri.sha256,"raw response original index/replay identity");
   statuses[row.status]=(statuses[row.status]||0)+1;
   need(["partial","no_match"].includes(row.status),"unexpected original status");
   for(const hit of row.hits){
    const f=input.get(hit.path),p=common.get(hit.path),e=hit.source_evidence;
    need(hit.evidence_valid===true && f&&p&&e&&e.source.encoding==="utf8","all source evidence valid");
    need(e.source.byte_len===p.bytes && e.source.content_digest===f.digest,"hit locked source metadata");
    need(same(e.span,hit.span)&&Number.isInteger(hit.span.start)&&Number.isInteger(hit.span.end)&&0<=hit.span.start&&hit.span.start<hit.span.end&&hit.span.end<=p.bytes,"source span bound");
    need(utf8(hit.text).length===hit.span.end-hit.span.start,"evidence slice byte length");
    need(/^[0-9a-f]{64}$/.test(e.slice_digest)&&/^[0-9a-f]{64}$/.test(e.source.snapshot_id),"evidence identity form");hits++;
   }
  }
  need(pairs.size===300&&rawPaths.size===300,"complete request matrix");
  const expectedStatus=repo==="cc-switch"?{partial:300}:{partial:270,no_match:30};
  need(same(statuses,expectedStatus),"preserve actual statuses");
  const reasons=normalized.filter(r=>r.status==="partial").map(r=>r.case_id+" Partial");
  need(gate.exit_code===1 && gate.status==="gate_failed" && same(gate.reasons,reasons),"exact original failure reasons");
  need(same(gate,observed.gate)&&observed.replay_exit_code===1,"original replay gate identity");
  need(metrics.invalid_hits===0 && metrics.unverified_hits===0,"no invalid evidence hidden");
  for(const name of ["top1","ndcg10"]){const mean=scores.reduce((a,s)=>{need(typeof s[name]==="number"&&Number.isFinite(s[name]),"finite original score");return a+s[name]},0)/300;need(Math.abs(mean-metrics["mean_"+name])<1e-12,"original scores aggregate");}
  let originalProfileFiles=0;
  for(const [name,f]of Object.entries(observed.raw_files)){const oi=original.get(dir+name);need(oi&&oi.bytes===f.bytes&&oi.sha256===f.sha256,"all replay-preserved raw members");originalProfileFiles++;}
  output.push({repo,profile,query_ids:100,repetitions:3,rows:300,unique_pairs:pairs.size,unique_raw_responses:rawPaths.size,statuses,hits,all_hit_evidence_valid:true,all_hit_paths_in_common_locked_source:true,all_hit_source_digests_and_spans_match_original_source_manifest:true,all_hit_utf8_slice_lengths_match:true,invalid_hits:metrics.invalid_hits,unverified_hits:metrics.unverified_hits,source_files:common.size,source_bytes:expected.common_bytes,source_digest:manifest.input.source_digest,query_digest:manifest.input.query_digest,query_sha256:lockSuite.query_sha256,suite_sha256:lockSuite.sha256,original_profile_files_bound:originalProfileFiles,gate_exit_code:1,replay_exit_code:1,top1:metrics.mean_top1,ndcg10:metrics.mean_ndcg10});
 }
}
return {source:packageManifest.product_source,original_files:original.size,original_roundtrip:roundtrip,profiles:output,total_rows:output.reduce((a,x)=>a+x.rows,0),total_hits:output.reduce((a,x)=>a+x.hits,0),scope:"Original complete engineering measurement, quality gate remains failed. No new candidate run, scoring, query insertion, or native semantic gold certification."};
}
if (typeof module !== 'undefined') module.exports = { auditOriginal };
