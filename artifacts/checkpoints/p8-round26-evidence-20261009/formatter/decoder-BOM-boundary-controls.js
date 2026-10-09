function rustfmtBOMBoundaryControls(decoderSource) {
 const api=new Function(decoderSource+"\nreturn {decode:decodeCandidateRustfmtSixFileLog,utf8Bytes,sha256Bytes,base64BytesEncode,canonicalFrameJSON,SYNTHETIC_SCHEMA,SYNTHETIC_FILES};")();
 const outs=Object.fromEntries(api.SYNTHETIC_FILES.map(k=>[k,k.endsWith(".json")?"{}\n":""]));
 const frames=[];
 for(const name of api.SYNTHETIC_FILES){const raw=api.utf8Bytes(outs[name]),sha256=api.sha256Bytes(raw).sha256,count=Math.ceil(raw.length/1024),common={schema:api.SYNTHETIC_SCHEMA,name};frames.push({...common,kind:"header",bytes:raw.length,sha256,count,raw_chunk_bytes:1024});for(let i=0;i<count;i++){const part=raw.slice(i*1024,(i+1)*1024);frames.push({...common,kind:"chunk",index:i,count,bytes:part.length,sha256:api.sha256Bytes(part).sha256,base64:api.base64BytesEncode(part)});}frames.push({...common,kind:"end",bytes:raw.length,sha256,count});}
 const clean=frames.map(f=>"2026-10-09T11:17:49.1234567Z "+api.canonicalFrameJSON(f)).join("\n");
 const bom=clean.split("\n").map((l,i)=>(i===0||i===7?"\uFEFF":"")+l).join("\n"),cases=[];
 const decoded=api.decode(bom);if(Object.entries(decoded.files).some(([k,v])=>v.text!==outs[k]))throw Error("BOM positive");
 cases.push({name:"single_BOM_before_timestamps_at_start_and_midstream_preserves_all_six_bodies",passed:true});
 for(const [name,fixture]of [["double_BOM_before_timestamp","\uFEFF\uFEFF"+clean],["BOM_in_JSON_payload",clean.replace('Z {','Z \uFEFF{')],["BOM_without_timestamp","\uFEFF"+clean.slice(clean.indexOf("{"))]]){let rejected=false;try{api.decode(fixture);}catch(e){rejected=true;}if(!rejected)throw Error(name);cases.push({name:name+"_rejected",passed:true});}
 return cases;
}
