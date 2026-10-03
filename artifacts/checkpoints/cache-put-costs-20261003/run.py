from pathlib import Path
import subprocess,json,hashlib,time
p=Path(__file__).resolve().parent
(p/'binary-receipt.json').write_text(json.dumps({name:{'path':str(p/'bin'/name),'sha256':hashlib.sha256((p/'bin'/name).read_bytes()).hexdigest(),'bytes':(p/'bin'/name).stat().st_size} for name in ['baseline','instrumented']},indent=2)+'\n')
rows=[]
for round_id in range(1,4):
 for name in (['baseline','instrumented'] if round_id%2 else ['instrumented','baseline']):
  root=p/'normalrun'/f'{name}-r{round_id}'
  assert not root.exists(), 'fresh run directory required'
  command=[str(p/'bin'/name),str(root),'1000']
  t=time.monotonic_ns()
  r=subprocess.run(command,capture_output=True,text=True)
  (p/f'{name}-r{round_id}.stdout.jsonl').write_text(r.stdout)
  (p/f'{name}-r{round_id}.stderr.log').write_text(r.stderr)
  receipt={'command':command,'exit_code':r.returncode,'process_ns':time.monotonic_ns()-t,'round':round_id}
  (p/f'{name}-r{round_id}.receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
  assert r.returncode==0, r.stderr
  for line in r.stdout.splitlines():
   row=json.loads(line);row['round']=round_id;rows.append(row)
  (p/'results.json').write_text(json.dumps(rows,indent=2)+'\n')
  print(json.dumps({'binary':name,'round':round_id,'put_ms':[(r['pass'],round(r['put_ns']/1e6,3)) for r in rows[-2:]]}),flush=True)
