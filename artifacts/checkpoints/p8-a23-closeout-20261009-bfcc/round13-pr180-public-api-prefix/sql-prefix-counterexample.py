"""SQL semantics only. Does not execute Rust product or any scale workload."""
import json, sqlite3, pathlib, re, hashlib, sys
ROOT=pathlib.Path('/workspace/scratch/bfccb8494ba0/pr180181-readonly/blobs')
RAM=pathlib.Path('/dev/shm/execution-recon-bfcc/pr180181/blobs')
schema=(RAM/'998608d9c6d07eb4afec0ce7e13c1f85bfc7a441').read_text()
test=(ROOT/'f698d3427ad90c194cfdb5b7c241e0802c1267d1').read_text()
ddl='\n'.join(re.search(r'CREATE TABLE IF NOT EXISTS '+table+r' \([\s\S]*?\n\);',schema).group(0) for table in ['files','resolution_dependencies'])
trigger=re.search(r'CREATE TRIGGER fail_dependency BEFORE INSERT ON resolution_dependencies[\s\S]*?END;',test).group(0)
assert "NEW.key='key-00070'" in trigger
rows=[('failed.py','name_bucket',f'key-{n:05}') for n in range(73)]
single='INSERT INTO resolution_dependencies(file_path,kind,key) VALUES(?1,?2,?3)'
head='INSERT INTO resolution_dependencies(file_path,kind,key) VALUES'
def run(kind):
    c=sqlite3.connect(':memory:'); c.execute('PRAGMA foreign_keys=ON'); c.executescript(ddl)
    c.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES('failed.py','python','fixture',0,0,'2026-10-09')")
    c.commit(); c.executescript(trigger); c.execute('BEGIN')
    statements=[]; caught=None
    try:
        if kind=='original_per_row':
            for row in rows:
                statements.append({'sql':single,'parameters':list(row)})
                c.execute(single,row)
        else:
            # Same exact 64/8/1 values_sql spelling and greedy tier order.
            # All 73 short rows are under the original 64 KiB bind bound.
            rest=rows
            for tier in (64,8,1):
                while len(rest)>=tier:
                    chunk,tail=rest[:tier],rest[tier:]
                    sql=head+','.join(['(?,?,?)']*tier)
                    params=[v for row in chunk for v in row]
                    statements.append({'sql':sql,'parameters':params})
                    c.execute(sql,params);rest=tail
    except sqlite3.DatabaseError as error:
        caught={'type':type(error).__name__,'message':str(error),'sqlite_errorcode':error.sqlite_errorcode,'sqlite_errorname':error.sqlite_errorname}
    assert caught and caught['message']=='injected dependency failure'
    in_transaction_after_error=c.in_transaction
    c.commit()
    retained=c.execute('SELECT rowid,file_path,kind,key FROM resolution_dependencies ORDER BY rowid').fetchall()
    return {'mode':kind,'caught':caught,'in_transaction_after_error':in_transaction_after_error,'commit_succeeded':not c.in_transaction,'retained_count':len(retained),'retained_rows':retained,'statements_attempted':statements}
results=[run('original_per_row'),run('proposed_64_8_1')]
assert [x['retained_count'] for x in results]==[70,64]
assert results[0]['retained_rows'][:64]==results[1]['retained_rows']
assert [r[-1] for r in results[0]['retained_rows'][64:]]==[f'key-{n:05}' for n in range(64,70)]
out={'scope':'SQL-only source-derived counterexample, not Rust product execution or performance evidence','sqlite_version':sqlite3.sqlite_version,'python_version':sys.version,'ddl_extracted_from_git_blob':'998608d9c6d07eb4afec0ce7e13c1f85bfc7a441','trigger_extracted_from_git_blob':'f698d3427ad90c194cfdb5b7c241e0802c1267d1','ddl':ddl,'trigger':trigger,'row_count':73,'same_rows_both_modes':rows,'results':results,'conclusion':'Both catch the same SQLITE_CONSTRAINT_TRIGGER and can commit; original retains 70 rows, batched retains 64. Old public API reaches batching in candidate #180, so byte-identical public method does not preserve this observable failure prefix.'}
print(json.dumps(out,sort_keys=True,indent=2))
