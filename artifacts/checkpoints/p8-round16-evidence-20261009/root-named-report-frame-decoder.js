function decodeNamedIntakeReports(log, expectedPaths) {
 const begin="BEGIN P8_COMPLETE_SAFE_INTAKE_JSON",end="END P8_COMPLETE_SAFE_INTAKE_JSON";
 const clean=l=>l.replace(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?Z /,"");
 const lines=log.split("\n"),groups=[];let start=-1;
 for(let i=0;i<lines.length;i++){const s=clean(lines[i]);if(s===begin){if(start!==-1)throw Error("nested report markers");start=i;}if(s===end){if(start===-1)throw Error("unpaired report end");groups.push(lines.slice(start,i+1).join("\n"));start=-1;}}
 if(start!==-1||groups.length!==expectedPaths.length||new Set(expectedPaths).size!==expectedPaths.length)throw Error("incomplete named report population");
 const expected=new Set(expectedPaths),result={};
 for(const g of groups){const lines=g.split("\n"),h=JSON.parse(clean(lines[1]));if(typeof h.profile!=="string"||!h.profile.startsWith("a23-soak:"))throw Error("unexpected report profile");const p=h.profile.slice(9);if(!expected.has(p)||Object.hasOwn(result,p))throw Error("unknown or duplicated named report");result[p]=decodeFullIntakeLog(g,"a23-soak:"+p);}
 if(Object.keys(result).length!==expected.size)throw Error("missing named report");return result;
}
