"use strict";
// Pure V8 transport decoder for exactly six candidate-rustfmt evidence files.
// It executes no receiver, subprocess, network, product or filesystem operation.
function sha256Bytes(input) {
 const bytes=Array.from(input);if(bytes.some(x=>!Number.isInteger(x)||x<0||x>255))throw Error('invalid byte');
 const length=bytes.length;bytes.push(128);while(bytes.length%64!==56)bytes.push(0);const bits=length*8;for(let i=7;i>=0;i--)bytes.push(i>=4?Math.floor(bits/2**(8*i))&255:(bits>>>8*i)&255);
 const k=[0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
 let h=[0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];const r=(x,n)=>(x>>>n)|(x<<(32-n));const w=new Int32Array(64);
 for(let off=0;off<bytes.length;off+=64){for(let i=0;i<16;i++){const j=off+4*i;w[i]=(bytes[j]<<24)|(bytes[j+1]<<16)|(bytes[j+2]<<8)|bytes[j+3];}for(let i=16;i<64;i++){const x=w[i-15],y=w[i-2];w[i]=(w[i-16]+(r(x,7)^r(x,18)^(x>>>3))+w[i-7]+(r(y,17)^r(y,19)^(y>>>10)))|0;}let[a,b,c,d,e,f,g,hh]=h;for(let i=0;i<64;i++){const t1=(hh+(r(e,6)^r(e,11)^r(e,25))+((e&f)^(~e&g))+k[i]+w[i])|0,t2=((r(a,2)^r(a,13)^r(a,22))+((a&b)^(a&c)^(b&c)))|0;hh=g;g=f;f=e;e=(d+t1)|0;d=c;c=b;b=a;a=(t1+t2)|0;}const v=[a,b,c,d,e,f,g,hh];h=h.map((x,i)=>(x+v[i])|0);}
 return {bytes:length,sha256:h.map(x=>(x>>>0).toString(16).padStart(8,"0")).join("")};
}

const SYNTHETIC_SCHEMA="p8-candidate-rustfmt-frame-v1";
const SYNTHETIC_FILES=["formatted-sources.json","commands.json","stderr.log","source-before.json","source-after.json","receipt.json"];
function requireFrame(ok,message){if(!ok)throw Error(message);}
function utf8Bytes(text){
 requireFrame(typeof text==="string","text required");
 const bytes=[];
 for(const ch of text){
  const cp=ch.codePointAt(0);
  requireFrame(cp<0xd800||cp>0xdfff,"lone UTF16 surrogate");
  if(cp<128)bytes.push(cp);
  else if(cp<2048)bytes.push(192|(cp>>6),128|(cp&63));
  else if(cp<65536)bytes.push(224|(cp>>12),128|((cp>>6)&63),128|(cp&63));
  else bytes.push(240|(cp>>18),128|((cp>>12)&63),128|((cp>>6)&63),128|(cp&63));
 }
 return bytes;
}
function strictUTF8(bytes){
 const out=[];
 for(let i=0;i<bytes.length;){
  const b=bytes[i++];let cp,n,min;
  if(b<128){out.push(String.fromCharCode(b));continue;}
  if(b>=0xc2&&b<=0xdf){cp=b&31;n=1;min=128;}
  else if(b>=0xe0&&b<=0xef){cp=b&15;n=2;min=2048;}
  else if(b>=0xf0&&b<=0xf4){cp=b&7;n=3;min=65536;}
  else throw Error("invalid UTF8 lead");
  requireFrame(i+n<=bytes.length,"truncated UTF8");
  while(n--){const c=bytes[i++];requireFrame(c>=128&&c<=191,"invalid UTF8 continuation");cp=(cp<<6)|(c&63);}
  requireFrame(cp>=min&&cp<=0x10ffff&&(cp<0xd800||cp>0xdfff),"noncanonical UTF8 codepoint");
  out.push(String.fromCodePoint(cp));
 }
 return out.join("");
}
function base64BytesEncode(bytes){
 const abc="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";let out="";
 for(let i=0;i<bytes.length;i+=3){
  const n=(bytes[i]<<16)|((bytes[i+1]||0)<<8)|(bytes[i+2]||0);
  out+=abc[(n>>>18)&63]+abc[(n>>>12)&63]+(i+1<bytes.length?abc[(n>>>6)&63]:"=")+(i+2<bytes.length?abc[n&63]:"=");
 }
 return out;
}
function strictBase64Bytes(text){
 requireFrame(typeof text==="string"&&text.length>0&&text.length<=1368&&text.length%4===0,"base64 length");
 requireFrame(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(text),"base64 syntax");
 const abc="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",out=[];
 for(let i=0;i<text.length;i+=4){
  const a=abc.indexOf(text[i]),b=abc.indexOf(text[i+1]),c=text[i+2]==="="?0:abc.indexOf(text[i+2]),d=text[i+3]==="="?0:abc.indexOf(text[i+3]);
  const n=(a<<18)|(b<<12)|(c<<6)|d;
  out.push((n>>>16)&255);if(text[i+2]!=="=")out.push((n>>>8)&255);if(text[i+3]!=="=")out.push(n&255);
 }
 requireFrame(base64BytesEncode(out)===text,"noncanonical base64 padding bits");
 return out;
}
function exactFrameKeys(frame,extra){
 const wanted=["schema","name",...extra].sort();
 requireFrame(frame&&typeof frame==="object"&&!Array.isArray(frame),"frame object");
 requireFrame(JSON.stringify(Object.keys(frame).sort())===JSON.stringify(wanted),"frame key set");
}
function canonicalFrameJSON(frame){
 const ordered={};for(const k of Object.keys(frame).sort())ordered[k]=frame[k];return JSON.stringify(ordered);
}
function decodeCandidateRustfmtSixFileLog(log){
 requireFrame(typeof log==="string"&&utf8Bytes(log).length<=32*1024*1024,"complete log bound");
 const frames=[];
 for(const original of log.split("\n")){
  const line=original.replace(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?Z /,"");
  if(!line.includes(SYNTHETIC_SCHEMA))continue;
  requireFrame(utf8Bytes(line).length<=2048,"physical JSON line bound");
  const frame=JSON.parse(line);
  requireFrame(frame.schema===SYNTHETIC_SCHEMA&&SYNTHETIC_FILES.includes(frame.name),"schema/name");
  // The fixed emitter uses sort_keys=True, compact separators and ASCII scalar values.
  // This exact form also refuses duplicate JSON keys, noncanonical numbers and escaping.
  requireFrame(canonicalFrameJSON(frame)===line,"noncanonical or duplicate-key frame");
  frames.push(frame);
 }
 requireFrame(frames.length>=12&&frames.length<=12*1024+5+12,"complete frame population");
 let at=0,totalRawBytes=0;const files={},metadata=[];
 for(const name of SYNTHETIC_FILES){
  const head=frames[at++];
  exactFrameKeys(head,["kind","bytes","sha256","count","raw_chunk_bytes"]);
  requireFrame(head.name===name&&head.kind==="header"&&head.raw_chunk_bytes===1024,"header/order");
  requireFrame(Number.isSafeInteger(head.bytes)&&head.bytes>=0&&head.bytes<=4*1024*1024,"file size bound");
  totalRawBytes+=head.bytes;requireFrame(totalRawBytes<=12*1024*1024,"cumulative raw file bound");
  requireFrame(Number.isSafeInteger(head.count)&&head.count===Math.ceil(head.bytes/1024),"header count");
  requireFrame(typeof head.sha256==="string"&&/^[a-f0-9]{64}$/.test(head.sha256),"whole digest");
  const raw=[];
  for(let i=0;i<head.count;i++){
   const chunk=frames[at++];
   exactFrameKeys(chunk,["kind","index","count","bytes","sha256","base64"]);
   const expected=Math.min(1024,head.bytes-i*1024);
   requireFrame(chunk.name===name&&chunk.kind==="chunk"&&chunk.index===i&&chunk.count===head.count,"chunk order/count");
   requireFrame(chunk.bytes===expected&&typeof chunk.sha256==="string"&&/^[a-f0-9]{64}$/.test(chunk.sha256),"chunk declared bytes/hash");
   const decoded=strictBase64Bytes(chunk.base64);
   requireFrame(decoded.length===expected&&sha256Bytes(decoded).sha256===chunk.sha256,"chunk raw bytes/hash");
   for(const byte of decoded)raw.push(byte);
  }
  const end=frames[at++];
  exactFrameKeys(end,["kind","bytes","sha256","count"]);
  requireFrame(end.name===name&&end.kind==="end"&&end.bytes===head.bytes&&end.count===head.count&&end.sha256===head.sha256,"end/header equality");
  const actual=sha256Bytes(raw);
  requireFrame(actual.bytes===head.bytes&&actual.sha256===head.sha256,"whole raw bytes/hash");
  const text=strictUTF8(raw);
  files[name]={raw:Uint8Array.from(raw),text,...actual,count:head.count};
  if(name.endsWith(".json"))files[name].json=JSON.parse(text);
  metadata.push({name,...actual,count:head.count});
 }
 requireFrame(at===frames.length,"extra or duplicate file/frame");
 return{schema:SYNTHETIC_SCHEMA,metadata,files,frame_count:frames.length,scope:"candidate-rustfmt-six-files-transport-only; no formatter-success or semantics inference"};
}
