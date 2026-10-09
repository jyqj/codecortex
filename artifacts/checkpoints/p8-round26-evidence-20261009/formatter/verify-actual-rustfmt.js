function verifyActualRustfmtDelivery(decoded, bundleBody, planBody, run, jobs, commit, hash256, gitHash1, api) {
 const require=(v,msg)=>{if(!v)throw Error(msg);};
 const equal=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
 const ident=(s)=>{const bytes=api.utf8Bytes(s).length;return {blob:gitHash1("blob "+bytes+"\0"+s),bytes,sha256:hash256(s)};};
 const checkIdent=(actual,expected,where)=>require(actual.blob===expected.blob&&actual.bytes===expected.bytes&&actual.sha256===expected.sha256,where);
 function decode64(s){if(s==="")return "";const bytes=[];for(let i=0;i<s.length;i+=1368){const part=api.strictBase64Bytes(s.slice(i,i+1368));bytes.push(...part);}return api.strictUTF8(bytes);}
 const plan=JSON.parse(planBody),bundle=JSON.parse(bundleBody);
 checkIdent(ident(bundleBody),plan.candidate_bundle,"fixed bundle");
 require(plan.candidates.length===15&&Object.keys(bundle.sources).length===15,"15 complete input bodies");
 const f=decoded.files,receipt=f["receipt.json"].json,cmd=f["commands.json"].json,formatted=f["formatted-sources.json"].json;
 require(run.id===37920790271&&run.run_attempt===1&&run.status==="completed"&&run.conclusion==="success","actual run");
 require(run.event==="push"&&run.head_branch===plan.branch&&run.path===".github/workflows/p8-candidate-rustfmt.yml","run trigger identity");
 require(jobs.total_count===1&&jobs.jobs.length===1,"one complete job");
 const job=jobs.jobs[0];
 require(job.id===113787941943&&job.head_sha===run.head_sha&&job.status==="completed"&&job.conclusion==="success","job identity and terminal");
 require(job.steps.find(s=>s.name==="Verify fixed byte inputs and format isolated copies").conclusion==="success","runner step actual 0");
 require(job.steps.find(s=>s.name==="Preserve all formatter originals and outputs").conclusion==="success","original artifact upload success");
 require(commit.sha===run.head_sha&&commit.parents.length===1&&commit.parents[0].sha===plan.parent,"actual controller parent");
 require(receipt.controller_commit===run.head_sha&&receipt.run_id===run.id&&receipt.attempt===1,"receipt actual run");
 require(receipt.schema==="p8-candidate-rustfmt-receipt-v1"&&receipt.status==="passed"&&receipt.formatted_count===15&&receipt.failed_or_not_run_count===0&&receipt.expected_candidates===15,"passed complete receipt");
 require(receipt.source_inputs_and_controller_unchanged===true&&receipt.new_measurements===0&&receipt.TODO_closed===0&&receipt.TODO_remaining===29,"scope and unchanged");
 require(f["source-before.json"].text===f["source-after.json"].text,"source snapshot bytes identical");
 const snap=f["source-before.json"].json;
 require(snap.controller_commit===run.head_sha&&snap.controller_parent===plan.parent&&snap.new_measurements===0,"snapshot controller");
 require(equal(Object.keys(snap.candidate_inputs).sort(),plan.candidates.map(x=>x.path).sort()),"snapshot exact 15 paths");
 require(equal(Object.keys(snap.controller_files).sort(),plan.controller_paths.slice().sort()),"snapshot exact controller paths");
 checkIdent(snap.controller_files["artifacts/checkpoints/p8-candidate-rustfmt-20261009/plan.json"],ident(planBody),"snapshot plan");
 checkIdent(snap.controller_files["artifacts/checkpoints/p8-candidate-rustfmt-20261009/candidate-sources.json"],ident(bundleBody),"snapshot bundle");
 checkIdent(snap.controller_files["artifacts/checkpoints/p8-candidate-rustfmt-20261009/run.py"],plan.runner,"snapshot runner");
 for(const name of plan.delivery.names.filter(x=>x!=="receipt.json"))checkIdent(ident(f[name].text),receipt.files_before_receipt[name],"receipt file "+name);
 require(formatted.schema==="p8-formatted-candidate-sources-v1"&&formatted.semantics_executed===false&&formatted.files.length===15,"formatted output schema");
 require(cmd.commands.length===18&&cmd.environment==="explicit token-free allowlist","18 commands");
 const commands=cmd.commands,tool=receipt.tool;
 require(equal(commands[0].argv,["rustup","which","--toolchain","1.95.0","rustfmt"]),"tool lookup");
 require(decode64(commands[0].stdout_base64).trim()===tool.path,"actual located tool");
 require(equal(commands[1].argv,[tool.path,"--version"])&&decode64(commands[1].stdout_base64)===tool.rustfmt_version,"formatter version");
 require(equal(commands[2].argv,["rustup","run","1.95.0","rustc","--version","--verbose"])&&decode64(commands[2].stdout_base64)===tool.rustc_version_verbose,"compiler version");
 require(tool.rustc_version_verbose.includes("release: 1.95.0\n")&&tool.edition==="2021"&&tool.input_mode==="stdin; no out-of-line module recursion","exact edition/toolchain/stdin");
 checkIdent(tool.empty_config,ident(""),"empty configuration");
 const cwd="/home/runner/work/_temp/p8-candidate-rustfmt-"+run.id, outputs=[];
 for(let i=0;i<commands.length;i++){const c=commands[i];require(c.exit_code===0&&c.cwd===cwd,"command exit/cwd "+i);for(const k of ["stdout","stderr"])checkIdent(ident(decode64(c[k+"_base64"])),c[k],"command raw "+k+" "+i);require(c.stderr.bytes===0,"empty percommand stderr "+i);}
 for(let i=0;i<15;i++){
  const expected=plan.candidates[i],entry=formatted.files[i],c=commands[i+3],input=bundle.sources[expected.path];
  require(entry.path===expected.path&&c.candidate_path===expected.path&&entry.command_index===i+3,"candidate order/command binding");
  checkIdent(ident(input),expected,"input body");checkIdent(entry.before,expected,"before identity");checkIdent(snap.candidate_inputs[expected.path],expected,"snapshot candidate");checkIdent(c.stdin,expected,"actual stdin");
  require(equal(c.argv,[tool.path,"--edition","2021","--emit","stdout","--config-path",cwd+"/empty-rustfmt.toml"]),"formatter argv");
  require(entry.exit_code===0&&entry.status==="formatted"&&typeof entry.content==="string"&&entry.content.length>0,"candidate result");
  require(decode64(entry.stdout_base64)===entry.content&&entry.stdout_base64===c.stdout_base64,"output full bytes equal command");
  checkIdent(ident(entry.content),entry.after,"output body");checkIdent(c.stdout,entry.after,"output raw metadata");
  outputs.push({path:entry.path,mode:"100644",input:entry.before,output:entry.after,command_index:i+3,changed:entry.before.blob!==entry.after.blob});
 }
 require(f["stderr.log"].text===commands.map((c,i)=>"command-"+i+"\n"+decode64(c.stderr_base64)+"\n").join(""),"combined stderr exact");
 return {status:"accepted_scoped_candidate_copy_formatting",run_id:run.id,job_id:job.id,controller:run.head_sha,parent:plan.parent,
   input_count:15,commands:18,format_exit0:15,changed_count:outputs.filter(x=>x.changed).length,input_bytes:plan.candidates.reduce((n,x)=>n+x.bytes,0),output_bytes:outputs.reduce((n,x)=>n+x.output.bytes,0),
   outputs,source_before_after_identical:true,tool,scope:"Actual formatter output custody only; no Cargo, compilation, Rust test methods, product workloads, old CI certification or TODO completion; artifact ZIP recorded by API but not downloaded."};
}
