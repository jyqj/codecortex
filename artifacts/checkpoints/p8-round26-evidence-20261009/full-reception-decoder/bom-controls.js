function full15BOMBoundaryControls(oldSource,newSource) {
 const make=source=>new Function(source+"\nreturn {decode:decodeFixed275eFullLog,utf8Bytes,sha256Bytes,base64BytesEncode,canonicalFrameJSON,SYNTHETIC_SCHEMA,SYNTHETIC_FILES};")();
 const old=make(oldSource),api=make(newSource),contents=Object.fromEntries(api.SYNTHETIC_FILES.map(n=>[n,n.endsWith(".json")?"{}\n":""]));
 const frames=[];
 for(const name of api.SYNTHETIC_FILES){const raw=api.utf8Bytes(contents[name]),sha256=api.sha256Bytes(raw).sha256,count=Math.ceil(raw.length/1024),common={schema:api.SYNTHETIC_SCHEMA,name};
 frames.push({...common,kind:"header",bytes:raw.length,sha256,count,raw_chunk_bytes:1024});
 for(let i=0;i<count;i++){const p=raw.slice(i*1024,(i+1)*1024);frames.push({...common,kind:"chunk",index:i,count,bytes:p.length,sha256:api.sha256Bytes(p).sha256,base64:api.base64BytesEncode(p)});}
 frames.push({...common,kind:"end",bytes:raw.length,sha256,count});}
 const clean=frames.map(f=>"2026-10-09T11:17:49.1234567Z "+api.canonicalFrameJSON(f)).join("\n");
 const cases=[],assert=(v,m)=>{if(!v)throw Error(m);},reject=(fn,v)=>{try{fn(v);}catch(e){return true;}return false;};
 const original=old.decode(clean),normal=api.decode(clean);
 assert(JSON.stringify(original.metadata)===JSON.stringify(normal.metadata),"no-BOM identity");
 cases.push({name:"all15_clean_small_fixture_old_and_new_metadata_and_bytes_equal",passed:true});
 const bom=clean.split("\n").map((line,i)=>(i===0||i===17?"\uFEFF":"")+line).join("\n");
 assert(reject(old.decode,bom),"old real boundary");cases.push({name:"old_decoder_rejects_start_and_midstream_BOM",passed:true});
 const got=api.decode(bom);for(const [name,f]of Object.entries(got.files))assert(f.text===contents[name],"full bytes");
 cases.push({name:"single_BOM_before_timestamp_at_start_and_midstream_all15_bytes_equal",passed:true});
 for(const [name,fixture] of [["double_prefix_BOM","\uFEFF\uFEFF"+clean],["payload_BOM",clean.replace('Z {','Z \uFEFF{')],["BOM_without_timestamp","\uFEFF"+clean.slice(clean.indexOf("{"))],["missing_final_end",bom.slice(0,bom.lastIndexOf("\n"))],["duplicate_final_end",bom+"\n"+bom.slice(bom.lastIndexOf("\n")+1)]]){
 assert(reject(api.decode,fixture),name);cases.push({name:name+"_rejected",passed:true});}
 return {schema:"p8-full15-BOM-boundary-controls-v1",cases,total:cases.length,all_passed:true,fixture_files:15,fixture_frames:frames.length,fixture_raw_bytes:Object.values(contents).reduce((n,s)=>n+api.utf8Bytes(s).length,0),fixture_log_bytes:api.utf8Bytes(bom).length,scope:"Synthetic transport only; original large24MiB controls not rerun; no receiver/native/product/network execution."};
}