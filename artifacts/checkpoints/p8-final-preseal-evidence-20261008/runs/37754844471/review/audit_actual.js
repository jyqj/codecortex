
function auditPreseal(read, inventory, schedule, sha256) {
 const need=(v,s)=>{if(!v)throw Error(s)}, stable=v=>JSON.stringify(v,Object.keys(v).sort());
 const canonical=v=>Array.isArray(v)?v.map(canonical):v&&typeof v==="object"?Object.fromEntries(Object.keys(v).sort().map(k=>[k,canonical(v[k])])):v;
 const eq=(a,b)=>JSON.stringify(canonical(a))===JSON.stringify(canonical(b));
 const json=(k,p)=>JSON.parse(read(k,p)), lines=(k,p)=>read(k,p).trim().split("\n").map(JSON.parse);
 const bypath=Object.fromEntries(inventory.synthetic.files.map(x=>[x.path,x]));
 const b=json("synthetic","before.json"),a=json("synthetic","after.json"),pb=json("prepare","before.json"),pr=json("prepare","result.json"),result=json("synthetic","result.json"),report=json("synthetic","replay/review.json");
 for(const k of ["candidate","helpers","environment","python_dependency_closure"]){need(eq(b[k],a[k]),"synthetic closure "+k);need(eq(pb[k],pr[k]),"prepare closure "+k);}
 need(read("synthetic","before.json")===read("prepare","before.json"),"cross job before bytes");
 need(b.candidate.commit==="2cd04485b0e0b23483f64671d56538bbaa47f442"&&b.candidate.tree==="4db46c8403c0a8687dc01f95ded006e31f842076"&&b.candidate.dirty===false,"source identity");
 const mapFacts={};for(const [n,m,count] of [["source",b.candidate.source,1038],["validation",b.candidate.validation,110],["helpers",b.helpers,6]]) {
   need(m.count===count&&Object.keys(m.entries).length===count,n+" count");
   const digest=sha256(JSON.stringify(canonical(m.entries),null,2)+"\n").sha256;
   need(digest===m.sha256,n+" full map hash");mapFacts[n]={count,sha256:digest};
 }
 need(b.helpers.sha256==="c9e27b4f838d9c39f00fe7af95953d5fc8c596c25ef6b0a1c4d93f9d30c87cb4","six fixed helper");
 for(const k of ["synthetic","prepare"])for(const role of ["product","evaluator"]){
  const input=json(k,"package/builds/"+role+"/source-inputs.json");
  need(Object.keys(input).length===1038,"build source count");for(const [path,hash]of Object.entries(input))need(b.candidate.source.entries[path]?.sha256===hash,"build source "+path);
 }
 const execution=json("synthetic","execution.json");
 for(const k of ["candidate","helpers"])need(eq(execution[k],b[k]),"execution closure "+k);
 need(eq(execution.source,a.source),"execution source complete");
 need(eq(result.execution_inventory,report.raw_inventory),"result/report full execution inventory");
 for(const [path,r]of Object.entries(report.raw_inventory)){const actual=bypath["runs/"+path];need(actual&&actual.bytes===r.bytes&&actual.sha256===r.sha256&&(actual.zip_mode&511)===r.mode,"full execution actual member "+path);}
 need(Object.keys(a.source).length===22,"all invented source files");for(const [path,r]of Object.entries(a.source)){const raw=read("synthetic","synthetic-inputs/source/"+path),actual=sha256(raw);need(actual.sha256===r.sha256&&actual.utf8_bytes===r.bytes,"actual source bytes "+path);}
 const modules=b.python_dependency_closure.modules;
 need(Object.keys(modules).length===147&&Object.values(modules).filter(x=>x.sha256).length===110,"complete actual module closure");
 const admission=json("prepare","source-admission.json"),lock=json("prepare","source-lock.json");
 need(Object.keys(admission.admitted).length===113&&Object.keys(admission.excluded).length===0,"113 admission");
 need(eq(Object.keys(admission.admitted).sort(),lock.files),"all source lock paths");
 need(lock.digest==="98a6ffa33befc33c843b8b3143c6795056a5e7617d60be64ec47643ab2fa0214","actual Rust BLAKE3");
 need(lock.commit===admission.commit&&lock.commit==="4e5faa3d7e4008d89e0d8bf1ea87b6d9a061a16d"&&admission.tree==="08234b584a09eb3893b061d2d7ea4b87608dfbf1","upstream identity");
 need(admission.policy.query_or_gold_based_filter===false&&admission.policy.Git_tracked_only===true&&admission.policy.max_bytes===512000,"generic source policy");
 need(pr.new_test_bodies===0&&pr.scheduled_product_queries===0&&pr.authoring_authorized===false&&pr.operational_index_readiness==="not_run","prepare scope");
 function receipt(k,p,code,timeout=600) {
  const r=json(k,p);
  need(r.process_exit_code===code&&r.effective_exit_code===code&&r.timed_out===false&&r.timeout_seconds===timeout,"actual command "+p);
  need(r.executable.sha256==="afdaf1ddbd28340f4ffbfbbef935194d563f3242d31ffb022b9bbd208e31537f","original evaluator "+p);
  for(const [f,rec]of Object.entries(r.streams)){const s=read(k,p.replace(/receipt.json$/,f));const hash=sha256(s);need(hash.sha256===rec.sha256&&hash.utf8_bytes===rec.bytes,"actual streams "+p+f);}
  return{path:p,process_exit_code:code,executable_sha256:r.executable.sha256};
 }
 const commands=[receipt("prepare","commands/source-lock-freeze/receipt.json",0),receipt("prepare","commands/source-lock-validate/receipt.json",0)];
 need(result.status==="synthetic_capability_verified"&&result.actual_scheduled_results===288&&result.synthetic_control_only===true&&result.clean_v1_count===0&&result.task_acceptance==="not_decided","synthetic scope");
 need(report.actual_result_schedule.length===288&&report.per_row.length===288&&schedule.requests.length===288,"complete 288");
 for(let i=0;i<288;i++)for(const [k,v]of Object.entries(schedule.requests[i]))need(report.actual_result_schedule[i][k]===v,"original actual schedule "+i+" "+k);
 const derived=["metrics.json","scores.jsonl","query-slices.json","costs.jsonl","latency-summary.json","latency-strata.json","resource-ledger.json","gate.json","report.md"];
 const arms=["candidate_local_default","rg_baseline"],status={},replay=[],facts={},allRows=[];
 let validHits=0;
 for(const arm of arms){
  const n=lines("synthetic","runs/"+arm+"/normalized.jsonl"),scores=lines("synthetic","runs/"+arm+"/scores.jsonl"),q=lines("synthetic","runs/"+arm+"/queries.jsonl"),gate=json("synthetic","runs/"+arm+"/gate.json");
  need(n.length===144&&scores.length===144&&q.length===48,"arm cardinality");
  need(gate.exit_code===0,"original gate zero");
  const s=report.actual_result_schedule.filter(x=>x.arm===arm),p=report.per_row.filter(x=>x.arm===arm),counts={};const pairs=new Set();
  for(let i=0;i<144;i++){
   const row=n[i],sch=s[i],rr=p[i],query=q[sch.input_row_index];
   need(query.id===row.case_id&&row.case_id===sch.case_id&&row.repetition===sch.repetition&&row.elapsed_us===sch.elapsed_us&&row.status===sch.status,"normalized schedule");
   need(rr.case_id===row.case_id&&rr.repetition===row.repetition&&eq(rr.native_score,scores[i]),"native original score identity");
   need(query.query_family===rr.family&&query.annotations.prospective_temporal_v2.variant===rr.variant&&query.category===rr.category,"report query metadata");
   const pair=row.case_id+":"+row.repetition;need(!pairs.has(pair),"duplicate result");pairs.add(pair);counts[row.status]=(counts[row.status]||0)+1;
   need(["success","no_match"].includes(row.status),"actual synthetic status");
   const raw=bypath["runs/"+arm+"/"+row.raw_path];need(raw&&raw.sha256===sch.raw_sha256,"raw schedule actual SHA");
   const copy=bypath["replay/"+arm+"-original-scorer-replay/"+row.raw_path];need(copy&&raw.bytes===copy.bytes&&raw.sha256===copy.sha256,"original replay raw identical");
   for(const hit of row.hits){need(hit.evidence_valid===true,"valid source evidence");need(Object.hasOwn(a.source,hit.path),"invented admitted source scope");validHits++;}
   allRows.push({arm,query,normalized:row,score:scores[i],report:rr});
  }
  need(eq(counts,report.row_status_counts[arm]),"reported status counts");status[arm]=counts;
  for(const file of [...derived,"manifest.json","queries.jsonl","normalized.jsonl"])need(read("synthetic","runs/"+arm+"/"+file)===read("synthetic","replay/"+arm+"-original-scorer-replay/"+file),"byte replay "+arm+"/"+file);
  commands.push(receipt("synthetic","replay/"+arm+"-replay-command/receipt.json",0),receipt("synthetic","replay/"+arm+"-raw-drift-command/receipt.json",2));
  need(read("synthetic","replay/"+arm+"-raw-drift-command/stderr.log").includes("raw response drift"),"raw drift message");
  need(read("synthetic","replay/"+arm+"-raw-drift-control/raw/000000.json")===read("synthetic","runs/"+arm+"/raw/000000.json")+"\n","exact owned drift");
  const reject=json("synthetic","all-no-match-control/"+arm+"/rejection.json");
  need(reject.status==="rejected"&&reject.original_raw_modified===false&&reject.new_product_queries===0,"all no match reject");
  replay.push({arm,original_gate:gate,derived_files_compared:9,input_files_compared:3,raw_files_compared:144});
  facts[arm]={rows:144,queries:48,pairs:pairs.size};
 }
 need(eq(status.candidate_local_default,{success:120,no_match:24})&&eq(status.rg_baseline,{success:60,no_match:84}),"actual statuses");
 const suite=json("synthetic","synthetic-inputs/inputs/suite.json");
 need(eq(Object.keys(a.source).sort(),suite.source.files),"same generic synthetic source scope");
 for(const [k,v]of Object.entries({repetitions:3,warmup:0,seed:20261003,timeout_ms:30000,top_k:10}))need(suite[k]===v,"original budget "+k);
 need(suite.scoring==="codecortex-native-v1"&&eq(suite.engine_config,{auto_index:{enabled:false},indexing:{include_hidden_files:true,include_text_files:true}}),"exact original config");
 for(const arm of arms)commands.push(receipt("synthetic","commands/"+arm+"/receipt.json",0,4800));
 const witnesses=json("synthetic","synthetic-positive-witnesses.json");need(witnesses.length===2,"two witness arms");
 let positive=0;for(const w of witnesses){need(w.target_path==="units/unit01.py"&&w.repetitions.length===3,"witness source");for(const rep of w.repetitions){
  const row=allRows.find(x=>x.arm===w.arm&&x.normalized.case_id===w.case_id&&x.normalized.repetition===rep.repetition);
  need(row&&row.normalized.hits.some(x=>x.path===w.target_path&&x.evidence_valid===true),"positive actual hit");
  need(sha256(read("synthetic","runs/"+w.arm+"/"+rep.raw_path)).sha256===rep.raw_sha256,"positive raw bytes");positive++;
 }}
 const mean=xs=>xs.some(x=>x===null||!Number.isFinite(x))?null:xs.reduce((a,b)=>a+b,0)/xs.length;
 const near=(x,y)=>x===null?y===null:Number.isFinite(x)&&Number.isFinite(y)&&Math.abs(x-y)<1e-12;
 for(const arm of arms)for(const [family,metrics]of Object.entries(report.family_means[arm])){
  const familyRows=allRows.filter(x=>x.arm===arm&&x.query.query_family===family);need(familyRows.length===12,"complete family denominator");
  for(const [metric,value]of Object.entries(metrics)){
   const vals=familyRows.map(x=>metric==="normalized_no_match_zero_hits"?(x.normalized.status==="no_match"&&x.normalized.hits.length===0?1:0):metric==="no_answer_correct"?(x.score[metric]===null?null:Number(x.score[metric])):x.score[metric]);
   need(near(mean(vals),value),"family mean "+metric);
  }
 }
 for(const [stratum,st]of Object.entries(report.strata)){const expected=stratum==="answerable"?10:2;need(st.components===expected&&st.rows_per_arm===expected*12,"stratum denominator");
  for(const [metric,m]of Object.entries(st.metrics)){
   for(const arm of arms){const families=Object.entries(report.family_means[arm]).filter(([f,v])=>Object.hasOwn(v,metric));need(families.length===expected,"all family components");need(near(mean(families.map(([f,v])=>v[metric])),m.arms[arm]),"arm full mean");
    const d=m.denominators[arm];need(d.planned_components===expected&&d.planned_rows===expected*12&&d.defined_components===expected,"fixed original denominator");
   }
   const vals=[];for(const [f,d]of Object.entries(m.paired_component_deltas)){const actual=report.family_means[arms[0]][f][metric]-report.family_means[arms[1]][f][metric];need(near(actual,d),"paired component delta");vals.push(d);}
   need(vals.length===expected&&near(mean(vals),m.bootstrap.mean_candidate_minus_rg),"full paired mean");
   need(m.bootstrap.independent_components===expected&&m.bootstrap.replicates===10000&&m.bootstrap.seed===20261003,"bootstrap fixed inputs");
  }
 }
 const controls=json("synthetic","negative-controls.json"),seal=json("synthetic","seal-controls/review.json"),reportControl=json("synthetic","report-contract-controls.json");
 need(controls.length===33,"complete controls");need(seal.controls.length===10&&seal.controls.every(x=>x.rejected===true)&&seal.real_service_chronology_verified===false&&seal.new_product_queries===0,"seal helper scope");
 const contractNames=["variant_gold","merged_component","invented_category","zero_overlap","missing_span","no_answer_scope","configuration","repetitions"];
 for(const name of contractNames)need(controls.some(x=>x.case===name&&x.rejected===true&&x.new_product_queries===0),"contract "+name);
 for(const name of ["query_drift","source_digest_drift"])commands.push(receipt("synthetic","negative-controls/"+name+"/command/receipt.json",2));
 const redirect=controls.find(x=>x.kind==="actual_loopback_HTTP_redirect_negative_and_reachable_sink_positive");
 need(redirect.redirect_status===302&&eq(redirect.counts,{redirect_source:1,sink:1,sink_authorization:[null]}),"real redirect and positive");
 need(reportControl.actual_measurement_evidence===false&&reportControl.new_product_queries===0&&reportControl.controls.length===10,"invented report controls");
 const nullcase=reportControl.controls.find(x=>x.case==="one_null_row_propagates_without_valid_subset");
 need(nullcase.family===null&&nullcase.arm===null&&nullcase.planned_components===10&&nullcase.planned_family_rows===12&&nullcase.paired.status==="inconclusive"&&nullcase.paired.defined_paired_components===9&&nullcase.paired.independent_components===10,"no valid subset/zero fill");
 need(reportControl.controls.find(x=>x.case==="original_gate1_partial_with_valid_scope").preserved===true,"gate1 valid bad quality preserved");
 return{status:"accepted_scoped_preseal_synthetic_evidence",maps:mapFacts,modules:147,file_backed_modules:110,full_before_after_equal:true,source_admission:{admitted:113,excluded:0,new_test_bodies:0,scheduled_product_queries:0,original_rust_blake3:lock.digest,independent_blake3_recomputation:false},rows:288,arms:facts,status_counts:status,valid_hits:validHits,real_positive_witnesses:positive,replay,actual_command_receipts:commands,aggregate:{families:12,answerable:10,no_answer:2,observations_per_family_arm:12,all_family_arm_means_and_paired_deltas_recomputed:true,bootstrap_input_counts_seed_checked:true,bootstrap_PRNG_independent_reexecution:false},controls:{all:33,contract:8,seal:10,report:10,actual_original_Rust_drift:2,local_redirect:1,configuration_budget_pre_dispatch:2},task_acceptance:false,protected_holdout_bodies_read:0};
}
