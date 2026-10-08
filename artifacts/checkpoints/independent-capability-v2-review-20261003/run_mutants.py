from pathlib import Path
import hashlib,json
from run_checks import run,R,T
H=R/'harness'; db=H/'crates/cc-db/src/capability_read.rs';server=H/'crates/cc-server/src/capability_status.rs'
mutants=[]
def mutant(name,path,old,new,package,test,features=[]):
 original=path.read_bytes();assert old.encode() in original
 changed=original.replace(old.encode(),new.encode(),1)
 path.write_bytes(changed)
 try:
  row=run(name,[T+'/cargo','test','-p',package,*features,'--lib',test,'--offline','--','--nocapture','--test-threads=1'],H,180)
  row['mutation_file']=str(path.relative_to(H));row['before_sha256']=hashlib.sha256(original).hexdigest();row['mutant_sha256']=hashlib.sha256(changed).hexdigest();row['old']=old;row['new']=new
  # A mutant only counts as killed if a compiled binary ran and semantic assertions failed.
  assert row['exit_code']==101 and row['binaries'] and 'test result: FAILED.' in (R/(name+'.log')).read_text(),row
  row['killed']=True;mutants.append(row)
 finally:path.write_bytes(original)
 (R/'mutants.json').write_text(json.dumps(mutants,indent=2)+'\n')
 print(name,row['exit_code'],'assertion killed',flush=True)
mutant('mutant-split-transaction',db,
 'let tx = conn.unchecked_transaction().map_err(db_err)?;\n    let generation = crate::read_generation::read_on(&tx)?;\n    observed_generation();',
 'let generation = crate::read_generation::read_on(conn)?;\n    observed_generation();\n    let tx = conn.unchecked_transaction().map_err(db_err)?;',
 'cc-db','independent_transaction_all_fields_and_actual_split_mutant')
# Real unsupported memory VFS checks must reject this historical fallback.
old=db.read_text();start=old.index('    match (code, moved) {');end=old.index('\n}\n\n#[cfg(test)]',start)
mutant('mutant-unsupported-failopen',db,old[start:end],
 '    match (code, moved) { (rusqlite::ffi::SQLITE_OK, 1) => Ok(false), _ => Ok(true) }',
 'cc-db','independent_has_moved_ffi_lifetime_and_unsupported_vfs')
mutant('mutant-drop-degradation-priority',server,
 'if !degradation.degraded {\n        return;\n    }',
 'if degradation.degraded {\n        return;\n    }',
 'cc-server','independent_active_configured_space_and_service_priority',['--features','semantic-http'])
# Rebuild restored positive cases because mutants overwrote the test binaries.
restored=[]
for name,args in [('restored-independent-db',['-p','cc-db','--lib','independent_v2']),('restored-independent-http',['-p','cc-server','--features','semantic-http','--lib','independent_v2'])]:
 row=run(name,[T+'/cargo','test',*args,'--offline','--','--nocapture','--test-threads=1'],H,300);assert row['exit_code']==0,row;restored.append(row);print(name,row['exit_code'],flush=True)
(R/'restored-runs.json').write_text(json.dumps(restored,indent=2)+'\n')
