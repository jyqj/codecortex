function sha256String(s) {
 const bytes=[];for(const c of s){const n=c.codePointAt(0);if(n<128)bytes.push(n);else if(n<2048)bytes.push(192|(n>>6),128|(n&63));else if(n<65536)bytes.push(224|(n>>12),128|((n>>6)&63),128|(n&63));else bytes.push(240|(n>>18),128|((n>>12)&63),128|((n>>6)&63),128|(n&63));}
 const length=bytes.length;bytes.push(128);while(bytes.length%64!==56)bytes.push(0);const bits=length*8;for(let i=7;i>=0;i--)bytes.push(i>=4?Math.floor(bits/2**(8*i))&255:(bits>>>8*i)&255);
 const k=[0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
 let h=[0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];const r=(x,n)=>(x>>>n)|(x<<(32-n));const w=new Int32Array(64);
 for(let off=0;off<bytes.length;off+=64){for(let i=0;i<16;i++){const j=off+4*i;w[i]=(bytes[j]<<24)|(bytes[j+1]<<16)|(bytes[j+2]<<8)|bytes[j+3];}for(let i=16;i<64;i++){const x=w[i-15],y=w[i-2];w[i]=(w[i-16]+(r(x,7)^r(x,18)^(x>>>3))+w[i-7]+(r(y,17)^r(y,19)^(y>>>10)))|0;}let[a,b,c,d,e,f,g,hh]=h;for(let i=0;i<64;i++){const t1=(hh+(r(e,6)^r(e,11)^r(e,25))+((e&f)^(~e&g))+k[i]+w[i])|0,t2=((r(a,2)^r(a,13)^r(a,22))+((a&b)^(a&c)^(b&c)))|0;hh=g;g=f;f=e;e=(d+t1)|0;d=c;c=b;b=a;a=(t1+t2)|0;}const v=[a,b,c,d,e,f,g,hh];h=h.map((x,i)=>(x+v[i])|0);}
 return {bytes:length,sha256:h.map(x=>(x>>>0).toString(16).padStart(8,"0")).join("")};
}

function base64EncodeAscii(s) {
 const alpha="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";let out="";
 for(let i=0;i<s.length;i+=3){const a=s.charCodeAt(i),b=i+1<s.length?s.charCodeAt(i+1):0,c=i+2<s.length?s.charCodeAt(i+2):0;if(a>127||b>127||c>127)throw Error("not ASCII");const n=(a<<16)|(b<<8)|c;out+=alpha[(n>>>18)&63]+alpha[(n>>>12)&63]+(i+1<s.length?alpha[(n>>>6)&63]:"=")+(i+2<s.length?alpha[n&63]:"=");}
 return out;
}

function base64DecodeAscii(s) {
 const alpha="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
 if(typeof s!=="string"||!s.length||s.length%4||!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(s))throw Error("invalid base64");
 let out="";for(let i=0;i<s.length;i+=4){const a=alpha.indexOf(s[i]),b=alpha.indexOf(s[i+1]),c=s[i+2]==="="?0:alpha.indexOf(s[i+2]),d=s[i+3]==="="?0:alpha.indexOf(s[i+3]);const n=(a<<18)|(b<<12)|(c<<6)|d;const vals=[(n>>>16)&255];if(s[i+2]!=="=")vals.push((n>>>8)&255);if(s[i+3]!=="=")vals.push(n&255);if(vals.some(x=>x>127))throw Error("report is not expected ASCII JSON bytes");out+=String.fromCharCode(...vals);}
 if(base64EncodeAscii(out)!==s)throw Error("noncanonical base64");return out;
}

function decodeFullIntakeLog(log, expectedProfile) {
 const ensure=(b,m)=>{if(!b)throw Error(m);};
 const keys=(o,want)=>ensure(o&&typeof o==="object"&&!Array.isArray(o)&&JSON.stringify(Object.keys(o).sort())===JSON.stringify([...want].sort()),"frame keys");
 const begin="BEGIN P8_COMPLETE_SAFE_INTAKE_JSON",end="END P8_COMPLETE_SAFE_INTAKE_JSON";
 const lines=log.split("\n").map(l=>l.replace(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?Z /,""));
 const b=lines.flatMap((l,i)=>l===begin?[i]:[]),e=lines.flatMap((l,i)=>l===end?[i]:[]);
 ensure(b.length===1&&e.length===1&&b[0]<e[0],"exact unique complete markers");
 const framed=lines.slice(b[0]+1,e[0]);ensure(framed.length>=3&&framed.every(l=>l.length>0&&l.length<24576),"complete bounded frame lines");
 const frames=framed.map(l=>JSON.parse(l));const header=frames[0],footer=frames.at(-1),headKeys=["kind","schema","profile","report_bytes","report_sha256","chunk_bytes","chunk_count"];
 keys(header,headKeys);keys(footer,headKeys);ensure(header.kind==="header"&&footer.kind==="complete"&&header.schema==="p8-safe-full-intake-log-frames-v1"&&header.profile===expectedProfile,"frame header identity");
 ensure(Number.isSafeInteger(header.report_bytes)&&header.report_bytes>0&&header.report_bytes<=64*1024**2&&header.chunk_bytes===16384&&Number.isSafeInteger(header.chunk_count)&&header.chunk_count===Math.ceil(header.report_bytes/16384),"frame declared population");
 ensure(/^[a-f0-9]{64}$/.test(header.report_sha256),"whole digest format");for(const k of headKeys.filter(k=>k!=="kind"))ensure(header[k]===footer[k],"footer differs");ensure(frames.length===header.chunk_count+2,"complete frame count");
 const pieces=[];for(let i=0;i<header.chunk_count;i++){const c=frames[i+1];keys(c,["kind","index","count","bytes","sha256","payload_base64"]);ensure(c.kind==="chunk"&&c.index===i&&c.count===header.chunk_count,"exact chunk order/population");const expectedBytes=Math.min(16384,header.report_bytes-i*16384);ensure(c.bytes===expectedBytes&&/^[a-f0-9]{64}$/.test(c.sha256),"chunk length/hash format");const piece=base64DecodeAscii(c.payload_base64);ensure(piece.length===c.bytes&&sha256String(piece).sha256===c.sha256,"chunk byte/hash identity");pieces.push(piece);}
 const body=pieces.join("");ensure(body.length===header.report_bytes&&sha256String(body).sha256===header.report_sha256,"whole report byte/hash identity");return {header,body,report:JSON.parse(body),chunks:pieces.length};
}
