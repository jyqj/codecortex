from pathlib import Path
import subprocess,os,json,datetime
E=Path(__file__).resolve().parent
order=[{'label':'baseline','scales':[1000,5000],'state':'already_complete_first','log':'baseline-pipeline.log'}]
for label,binary in [('candidate','candidate'),('candidate-repeat','candidate'),('baseline-repeat','baseline')]:
 row={'label':label,'scales':[1000,5000],'binary':binary,'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'state':'running'};order.append(row)
 (E/'run-order.json').write_text(json.dumps(order,indent=2)+'\n')
 with (E/(label+'-pipeline.log')).open('wb') as log:
  result=subprocess.run(['python3',str(E/'pipeline.py')],env={**os.environ,'FIFO_BINARY':str(Path('/workspace/scratch/bounded-parallel-ab')/binary),'PHASE_CASE_PREFIX':label},stdout=log,stderr=subprocess.STDOUT)
 row.update(exit_code=result.returncode,finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),state='complete' if result.returncode==0 else 'failed')
 (E/'run-order.json').write_text(json.dumps(order,indent=2)+'\n')
 if result.returncode:raise SystemExit(result.returncode)
