function runFullReceptionDecoderControls(api,includeCapacity){
 const results=[],names=api.SYNTHETIC_FILES,schema="p8-fixed275e-full-reception-frame-v1";
 function check(ok,message){if(!ok)throw Error(message);}
 function okay(name,f){const v=f();results.push({name,status:"passed",...v});}
 function rejects(name,f,pattern){let caught=null;try{f();}catch(e){caught=e.message;}check(caught!==null&&(!pattern||pattern.test(caught)),name+": missing or wrong refusal "+caught);results.push({name,status:"passed",refused:caught});}
 function files(overrides={}){return Object.fromEntries(names.map(name=>[name,overrides[name]??(name.endsWith(".json")?"{}\n":name==="prepare/stdout.log"?"x".repeat(1023)+"中😀\n":"")]));}
 function lines(items){
  const out=[];
  for(const name of names){const value=items[name],raw=value instanceof Uint8Array?value:api.utf8Bytes(value),whole=api.sha256Bytes(raw),count=Math.ceil(raw.length/1024),common={schema,name};
   out.push(api.canonicalFrameJSON({...common,kind:"header",...whole,count,raw_chunk_bytes:1024}));
   for(let i=0;i<count;i++){const part=raw.subarray(i*1024,Math.min(raw.length,(i+1)*1024));out.push(api.canonicalFrameJSON({...common,kind:"chunk",index:i,count,...api.sha256Bytes(part),base64:api.base64BytesEncode(part)}));}
   out.push(api.canonicalFrameJSON({...common,kind:"end",...whole,count}));
  }
  return out;
 }
 function modify(source,name,kind,change){let found=false;const out=source.map(line=>{const f=JSON.parse(line);if(!found&&f.name===name&&f.kind===kind){found=true;return api.canonicalFrameJSON(change(f));}return line;});check(found,"fixture target missing");return out;}
 const items=files(),base=lines(items),log=base.map(x=>"2026-10-09T00:00:00.0000000Z "+x).join("\n")+"\n";
 okay("all15_exact_population_unicode_chunk_split_empty_logs",()=>{const d=api.decodeFixed275eFullLog("\ufeffworkflow prelude\n"+log+"workflow epilogue\n");check(d.metadata.length===15&&Object.keys(d.files).length===15,"population");for(const n of names)check(d.files[n].text===items[n],"raw text differs");check(d.frame_count===base.length,"frame count");return{files:15,frames:d.frame_count,raw_bytes:d.total_raw_bytes};});
 rejects("missing_last_frame",()=>api.decodeFixed275eFullLog(base.slice(0,-1).join("\n")),/missing frame/);
 rejects("extra_duplicate_file",()=>api.decodeFixed275eFullLog(base.concat(base[0]).join("\n")),/extra or duplicate/);
 rejects("wrong_file_order",()=>api.decodeFixed275eFullLog(modify(base,names[0],"header",f=>({...f,name:names[1]})).join("\n")),/header\/order/);
 rejects("ordinary_file_4MiB_plus1",()=>api.decodeFixed275eFullLog(modify(base,names[0],"header",f=>({...f,bytes:4*1024*1024+1,count:4097})).join("\n")),/file size bound/);
 rejects("matrix_file_16MiB_plus1",()=>api.decodeFixed275eFullLog(modify(base,"replay/independent-matrix/matrix.json","header",f=>({...f,bytes:16*1024*1024+1,count:16385})).join("\n")),/file size bound/);
 rejects("chunk_index_changed",()=>api.decodeFixed275eFullLog(modify(base,"prepare/stdout.log","chunk",f=>({...f,index:1})).join("\n")),/chunk order\/count/);
 rejects("chunk_digest_changed",()=>api.decodeFixed275eFullLog(modify(base,names[0],"chunk",f=>({...f,sha256:"0".repeat(64)})).join("\n")),/chunk raw bytes\/hash/);
 const duplicate=base.slice();duplicate[0]=duplicate[0].replace('"bytes":3','"bytes":3,"bytes":3');
 rejects("duplicate_JSON_key",()=>api.decodeFixed275eFullLog(duplicate.join("\n")),/noncanonical or duplicate-key/);
 const padded=lines(files({"prepare/stdout.log":"a"}));
 rejects("noncanonical_base64_padding_bits",()=>api.decodeFixed275eFullLog(modify(padded,"prepare/stdout.log","chunk",f=>({...f,base64:"YR=="})).join("\n")),/noncanonical base64 padding bits/);
 rejects("invalid_UTF8_raw",()=>api.decodeFixed275eFullLog(lines(files({"prepare/stdout.log":new Uint8Array([255])})).join("\n")),/invalid UTF8 lead/);
 const whole=base.map(line=>{const f=JSON.parse(line);return f.name===names[0]&&f.kind!=="chunk"?api.canonicalFrameJSON({...f,sha256:"0".repeat(64)}):line;});
 rejects("whole_digest_changed_with_consistent_header_end",()=>api.decodeFixed275eFullLog(whole.join("\n")),/whole raw bytes\/hash/);
 rejects("lone_UTF16_surrogate_log",()=>api.decodeFixed275eFullLog("\ud800"+log),/lone UTF16 surrogate/);
 if(includeCapacity){
  const size=16*1024*1024,prefix='{"payload":"',suffix='"}\n',matrix=prefix+"x".repeat(size-prefix.length-suffix.length)+suffix;
  const large=files({"replay/independent-matrix/matrix.json":matrix,"prepare/stdout.log":"x".repeat(4*1024*1024),"prepare/stderr.log":"y".repeat(4*1024*1024-30)});
  let total=0;for(const x of Object.values(large))total+=api.utf8Bytes(x).length;check(total===24*1024*1024,"capacity fixture total");
  const largeLines=lines(large),largeLog=largeLines.join("\n")+"\n";
  okay("actual16MiB_matrix_and_exact24MiB_total_complete_decode",()=>{const d=api.decodeFixed275eFullLog(largeLog);check(d.total_raw_bytes===24*1024*1024&&d.files["replay/independent-matrix/matrix.json"].bytes===size&&d.files["replay/independent-matrix/matrix.json"].json.payload.length===size-prefix.length-suffix.length,"full capacity reconstruction");for(const n of names)check(d.files[n].text===large[n],"full capacity text differs");return{files:15,frames:d.frame_count,raw_bytes:d.total_raw_bytes,matrix_bytes:size,log_bytes:api.utf8Bytes(largeLog).length,matrix_sha256:d.files["replay/independent-matrix/matrix.json"].sha256};});
  rejects("actual24MiB_prefix_then_total_plus1",()=>api.decodeFixed275eFullLog(modify(largeLines,"replay/receipt.json","header",f=>({...f,bytes:4})).join("\n")),/total raw file bound/);
 }
 return{schema:"p8-full15-V8-transport-controls-v1",status:"passed",tests:results.length,results,scope:"actual in-memory V8 transport controls; no Python wrapper execution, real artifacts, product or measurements"};
}