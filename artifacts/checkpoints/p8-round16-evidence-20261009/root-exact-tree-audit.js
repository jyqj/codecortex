async function auditExactTreeChange(oldSha,newSha,planned,getTree){
 const cache=new Map();
 async function tree(sha){if(!sha)return [];if(!cache.has(sha))cache.set(sha,(async()=>{const t=await getTree(sha);if(t.truncated||t.sha!==sha||!Array.isArray(t.tree))throw Error("invalid or incomplete tree response");return t.tree;})());return cache.get(sha);}
 const diffs=[];
 async function walk(a,b,prefix){if(a===b)return;const rs=await Promise.allSettled([tree(a),tree(b)]);for(const r of rs)if(r.status!=="fulfilled")throw r.reason;const x=new Map(rs[0].value.map(e=>[e.path,e])),y=new Map(rs[1].value.map(e=>[e.path,e]));const pending=[];
 for(const n of new Set([...x.keys(),...y.keys()])){const before=x.get(n),after=y.get(n);if(before&&after&&before.sha===after.sha&&before.type===after.type&&before.mode===after.mode)continue;const path=prefix+n;
 if((!before||before.type==="tree")&&(!after||after.type==="tree"))pending.push(walk(before?.sha,after?.sha,path+"/"));else{if(before?.type==="tree"||after?.type==="tree")throw Error("tree/leaf type changed");diffs.push({path,before:before??null,after:after??null});}}
 for(const r of await Promise.allSettled(pending))if(r.status!=="fulfilled")throw r.reason;
 }
 await walk(oldSha,newSha,"");const expected=new Map(planned.map(e=>[e.path,e]));if(expected.size!==planned.length||diffs.length!==expected.size)throw Error("complete changed leaf population mismatch");
 for(const d of diffs){const e=expected.get(d.path);if(!e||!d.after||d.after.sha!==e.sha||d.after.type!==e.type||d.after.mode!==e.mode)throw Error("unexpected changed leaf "+d.path);}
 return {old_tree:oldSha,new_tree:newSha,exact_leaves:diffs.length,all_other_entries_preserved:true,all_tree_responses_complete:true,tree_objects:cache.size,diffs:diffs.sort((a,b)=>a.path.localeCompare(b.path))};
}
