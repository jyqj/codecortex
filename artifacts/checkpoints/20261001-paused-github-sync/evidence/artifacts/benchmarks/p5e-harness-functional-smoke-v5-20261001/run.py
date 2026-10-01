import pathlib,json,subprocess,os
root=pathlib.Path.cwd();base=root/'artifacts/benchmarks/p5e-harness-functional-smoke-v5-20261001';h=root/'artifacts/benchmarks/p5e-harness-20261001/final-v5';driver=h/'binaries/p5e-evidence-driver'
plan=json.load(open(h/'inputs/fanout-plan.json'));plan['repetitions']=3;plan['scope']='non-performance functional protocol/source/replay smoke;3 wholequeries,notformal300';short=base/'fanout-3-functional-plan.json';assert not short.exists();short.write_text(json.dumps(plan,indent=2)+'\n')
binaries={'baseline':root/'artifacts/benchmarks/p5e-baseline-release-20261001/binaries/codecortex','candidate':root/'artifacts/benchmarks/p5e-candidate-release-20261001-v2/binaries/codecortex'};env=os.environ.copy();env['CODECORTEX_BENCH_PROCESS_PROBE']='1';receipts=[]
for name,binary in binaries.items():
 for label,p in [('typed5',h/'inputs/graph-plan.json'),('fanout3',short)]:
  out=base/(name+'-'+label);log=base/(name+'-'+label+'.log');assert not log.exists()
  with log.open('w') as stream:rc=subprocess.call([str(driver),'graph',str(binary),str(p),str(out)],env=env,stdout=stream,stderr=subprocess.STDOUT)
  replay=base/(name+'-'+label+'-replay.json')
  replayrc=subprocess.call([str(driver),'replay-graph',str(p),str(out),str(replay)],env=env)
  receipts.append({'variant':name,'mode':label,'process_exit_code':rc,'replay_exit_code':replayrc,'summary':json.load(open(out/'summary.json'))})
  print(name,label,'terminal',rc,'replay',replayrc,flush=True)
(base/'FUNCTIONAL-RECEIPT.json').write_text(json.dumps({'status':'functional_only_not_G5','runs':receipts,'formal_matrix_not_run':True},indent=2)+'\n')
