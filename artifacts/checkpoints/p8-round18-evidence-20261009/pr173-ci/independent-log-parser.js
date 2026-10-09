#!/usr/bin/env node
// Read-only replay of original PR173 CI counts. No subprocess, network, or writes.
// Usage: node verify-pr173-ci-counts.mjs PR173-check.log PR170-review.json PR170-check.log
import fs from "node:fs";
import crypto from "node:crypto";
const expectedInputs = [
  ["PR173 original check log", "e054ef90af95bd936a1a408d1f118735c1c0ffdf718c281c782ee2afa7c988c5"],
  ["PR170 independent review", "58b4822249f4574f9384ebd92316d5a87bf46e23f16717d159ef6e744008f4d7"],
  ["PR170 original check log", "a7ec31c417cb2099cea629fb18f7459af392e19f3f234f873da5d52d68b9394e"],
];
function requireCondition(value, message) { if (!value) throw new Error(message); }
requireCondition(process.argv.length === 5, "Expected exactly three original input files");
const inputs = process.argv.slice(2).map((path, i) => {
  const bytes = fs.readFileSync(path);
  const sha256 = crypto.createHash("sha256").update(bytes).digest("hex");
  requireCondition(sha256 === expectedInputs[i][1], "Original input SHA mismatch: " + expectedInputs[i][0]);
  return {label: expectedInputs[i][0], bytes: bytes.length, sha256, text: bytes.toString("utf8")};
});
const parseOriginalPythonSuites = function parseOriginalPythonSuites(raw, expected){
 const lines=raw.replace(/\u001b\[[0-?]*[ -/]*[@-~]/g,"").split("\n").map((s,i)=>({line:i+1,text:s.replace(/^\ufeff/,"").replace(/^\d{4}-\d{2}-\d{2}T[0-9:.]+Z /,"").replace(/\r$/,"")}));
 const ends=lines.filter(x=>/^Ran (46|409|181) tests in [0-9.]+s$/.test(x.text));
 if(ends.length!==3)throw Error("expected three original Python summaries");
 let lower=0;const out=[];
 for(let i=0;i<expected.length;i++){
  const e=expected[i],end=ends[i],match=end.text.match(/^Ran (\d+) tests in ([0-9.]+)s$/);
  if(+match[1]!==e.count)throw Error("count mismatch");
  const rows=[];let pending=null;
  for(const item of lines.filter(x=>x.line>lower&&x.line<end.line)){
   const m=item.text.match(/^(test_[^ ]+) \(([^)]+)\) \.\.\. ?(.*)$/); if(m){
    if(pending)throw Error("unfinished top level before next method");
    pending={method:m[2],label:m[1],line:item.line,status:null};
    if(m[3]==="ok"){pending.status="ok";rows.push(pending);pending=null;}
    else if(/^(FAIL|ERROR|skipped)/.test(m[3]))throw Error("non-ok top level "+m[2]);
   }else if(pending&&item.text==="ok"){pending.status="ok";rows.push(pending);pending=null;}
   else if(pending&&/^(FAIL|ERROR|skipped)/.test(item.text))throw Error("non-ok pending "+pending.method);
  }
  if(pending||rows.length!==e.count||new Set(rows.map(x=>x.method)).size!==e.count)throw Error("population mismatch "+e.suite+" "+rows.length);
  const wanted=e.population.map(x=>x.method).sort();if(JSON.stringify(rows.map(x=>x.method).sort())!==JSON.stringify(wanted))throw Error("method ID population changed");
  const following=lines.filter(x=>x.line>end.line&&x.line<end.line+6).map(x=>x.text).filter(x=>x.trim());
  if(!following.includes("OK"))throw Error("no original OK");
  out.push({suite:e.suite,command:e.command,summary:end.text,line:end.line,count:rows.length,unique:rows.length,failures:0,errors:0,skips:0,one_original_invocation:true,population:rows});lower=end.line;
 } return out;
};
const prior = JSON.parse(inputs[1].text);
const suites = parseOriginalPythonSuites(inputs[0].text, prior.python_suites);
const clean = raw => raw.replace(/\u001b\[[0-?]*[ -/]*[@-~]/g, "").split("\n")
 .map((s,i) => ({line:i+1,text:s.replace(/^\ufeff/,"").replace(/^\d{4}-\d{2}-\d{2}T[0-9:.]+Z /,"").replace(/\r$/,"")}));
const rows = clean(inputs[0].text);
const previousRows = clean(inputs[2].text);
const groups = rows.filter(r=>r.text.startsWith("##[group]")).map(r=>r.text);
const previousGroups = previousRows.filter(r=>r.text.startsWith("##[group]")).map(r=>r.text);
requireCondition(groups.length === 54 && JSON.stringify(groups) === JSON.stringify(previousGroups), "Original full command group mismatch");
const records = [];
for (const row of rows) {
  if (!row.text.startsWith("{")) continue;
  try { records.push({line:row.line,value:JSON.parse(row.text)}); } catch {}
}
const proofs = records.filter(r=>r.value.schema_version===10 && r.value.source_version==="p8-completion-source-20261009-v15" && r.value.status==="passed");
requireCondition(proofs.length===1, "Selected original v15 result population differs");
const expectedProof = {
  "schema_version": 10,
  "source_version": "p8-completion-source-20261009-v15",
  "previous_source_version": "p8-oracle-compat-source-20261008-v14",
  "base_source": "7354db236c9d9850a75f31672697ae9eab44565e",
  "product_source": "a402e88d460179afb12e3e2dc066d62cf168c690",
  "review_source": "f66daf6088f6f6a8d990d736ee0c1213032ca175",
  "scope": "independently_reviewed_source_and_validation_inputs",
  "runtime_and_quality_claims": "require_separate_execution_evidence",
  "status": "passed",
  "complete_inputs": 1089,
  "executed_previous_proof": "p8-oracle-compat-source-20261008-v14"
};
requireCondition(JSON.stringify(proofs[0].value)===JSON.stringify(expectedProof), "Original selected proof differs");
const history = records.filter(r=>r.value.verifier_version==="historical-integrations-v2");
requireCondition(history.length===1 && JSON.stringify(history[0].value)===JSON.stringify(prior.historical_integrity_proof), "Original historical proof differs");
const rust = {summary_lines:0,passed_executions:0,failed_executions:0,ignored_executions:0,measured_executions:0};
for (const row of rows) {
 const m=row.text.match(/^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in /);
 if (!m) continue;
 rust.summary_lines++; rust.passed_executions+=+m[1]; rust.failed_executions+=+m[2]; rust.ignored_executions+=+m[3]; rust.measured_executions+=+m[4];
}
requireCondition(JSON.stringify(rust)===JSON.stringify({summary_lines:418,passed_executions:3281,failed_executions:0,ignored_executions:132,measured_executions:0}), "Original Rust execution summaries differ");
console.log(JSON.stringify({
 schema:"PR173-original-CI-read-only-log-count-replay-v1",
 status:"passed",
 scope:"Original fixed log counts and selected proof only; API/checkout/tree and security/MSRV are separately verified in the fixed CI review. Rust totals are executions, not unique cases.",
 input: inputs.map(({label,bytes,sha256})=>({label,bytes,sha256})),
 suites, selected_v15:proofs[0], historical_v2:history[0], rust, original_command_group_count:groups.length
},null,2));
