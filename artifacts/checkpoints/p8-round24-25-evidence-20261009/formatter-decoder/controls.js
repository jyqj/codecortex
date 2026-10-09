function runCandidateRustfmtDecoderControls(decoderSource) {
 const api = new Function(decoderSource+"\nreturn {decode:decodeCandidateRustfmtSixFileLog,utf8Bytes,sha256Bytes,base64BytesEncode,canonicalFrameJSON,SYNTHETIC_SCHEMA,SYNTHETIC_FILES};")();
 const {decode,utf8Bytes,sha256Bytes,base64BytesEncode,canonicalFrameJSON,SYNTHETIC_SCHEMA,SYNTHETIC_FILES}=api;
 const outputs={
  "formatted-sources.json":JSON.stringify({schema:"synthetic-only",files:[{content:"fn main() { let _ = \"λ🙂汉字\"; }\n"}],padding:"a".repeat(2080)})+"\n",
  "commands.json":JSON.stringify({commands:[],scope:"synthetic-only"})+"\n",
  "stderr.log":"",
  "source-before.json":"{\"source\":\"fixture\"}\n",
  "source-after.json":"{\"source\":\"fixture\"}\n",
  "receipt.json":"{\"status\":\"failed\",\"scope\":\"synthetic-only\"}\n"
 };
 function framesFor(name,raw){
  const hash=sha256Bytes(raw).sha256,count=Math.ceil(raw.length/1024);
  const common={schema:SYNTHETIC_SCHEMA,name};
  const list=[{...common,kind:"header",bytes:raw.length,sha256:hash,count,raw_chunk_bytes:1024}];
  for(let i=0;i<count;i++){const part=raw.slice(i*1024,(i+1)*1024);list.push({...common,kind:"chunk",index:i,count,bytes:part.length,sha256:sha256Bytes(part).sha256,base64:base64BytesEncode(part)});}
  list.push({...common,kind:"end",bytes:raw.length,sha256:hash,count});return list;
 }
 const frames=SYNTHETIC_FILES.flatMap(name=>framesFor(name,utf8Bytes(outputs[name])));
 const serialize=frames=>frames.map(canonicalFrameJSON).join("\n")+"\n";
 const clone=()=>JSON.parse(JSON.stringify(frames));
 const cases=[];
 function check(name,fn){fn();cases.push({name,status:"passed"});}
 function reject(name,mutate){check(name,()=>{let threw=false;try{decode(mutate());}catch(e){threw=true;}if(!threw)throw Error(name+": accepted");});}
 check("six_files_unicode_multichunk_empty_stderr_exact_bytes",()=>{
  const result=decode(serialize(frames));
  if(result.metadata.length!==6)throw Error("population");
  for(const name of SYNTHETIC_FILES){if(result.files[name].text!==outputs[name])throw Error("body");}
 });
 check("timestamps_and_unrelated_log_are_allowed",()=>{
  const log="ordinary setup line\n"+frames.map(f=>"2026-10-09T12:34:56.123456Z "+canonicalFrameJSON(f)).join("\n")+"\nfinished\n";
  if(decode(log).metadata.length!==6)throw Error("timestamp");
 });
 check("failed_receipt_decodes_without_claiming_success",()=>{
  if(decode(serialize(frames)).files["receipt.json"].json.status!=="failed")throw Error("status");
 });
 reject("missing_chunk_rejected",()=>{const x=clone();x.splice(1,1);return serialize(x);});
 reject("duplicate_complete_file_rejected",()=>serialize(frames.concat(framesFor(SYNTHETIC_FILES[0],utf8Bytes(outputs[SYNTHETIC_FILES[0]])))));
 reject("wrong_file_order_rejected",()=>{const x=clone();[x[0].name,x[4].name]=["commands.json","formatted-sources.json"];return serialize(x);});
 reject("wrong_schema_rejected",()=>serialize(frames.map(f=>({...f,schema:"different-schema"}))));
 reject("changed_chunk_hash_rejected",()=>{const x=clone();x[1].sha256="0".repeat(64);return serialize(x);});
 reject("end_header_mismatch_rejected",()=>{const x=clone();x.find(f=>f.kind==="end").bytes++;return serialize(x);});
 reject("duplicate_frame_keys_rejected",()=>{const lines=frames.map(canonicalFrameJSON);lines[0]=lines[0].replace("{","{\"kind\":\"header\",");return lines.join("\n");});
 reject("per_file_4m_plus_one_rejected_before_allocation",()=>{const x=clone();x[0].bytes=4*1024*1024+1;x[0].count=Math.ceil(x[0].bytes/1024);return serialize(x);});
 reject("fractional_header_count_rejected",()=>{const x=clone();x[0].count=3.5;return serialize(x);});
 reject("invalid_utf8_rejected_with_valid_transport_hashes",()=>{
  return serialize(SYNTHETIC_FILES.flatMap(name=>framesFor(name,name==="stderr.log"?[0xc0,0xaf]:utf8Bytes(outputs[name]))));
 });
 reject("noncanonical_base64_padding_rejected",()=>{
  const x=SYNTHETIC_FILES.flatMap(name=>framesFor(name,name==="stderr.log"?[0]:utf8Bytes(outputs[name])));
  x.find(f=>f.name==="stderr.log"&&f.kind==="chunk").base64="AB==";return serialize(x);
 });
 reject("invalid_json_body_rejected_with_valid_transport_hashes",()=>{
  return serialize(SYNTHETIC_FILES.flatMap(name=>framesFor(name,utf8Bytes(name==="commands.json"?"{":outputs[name]))));
 });
 return {schema:"p8-candidate-rustfmt-decoder-small-controls-v1",scope:"synthetic transport only; no rustfmt, Cargo, tests or product execution",
  tests:cases.length,cases,all_passed:true,fixture_raw_bytes:Object.values(outputs).reduce((n,s)=>n+utf8Bytes(s).length,0),
  fixture_log_bytes:utf8Bytes(serialize(frames)).length,fixture_frames:frames.length,
  unexecuted_capacity_boundaries:["12MiB cumulative maximum","32MiB log maximum","12305 maximum frame population","4MiB positive maximum"]};
}
