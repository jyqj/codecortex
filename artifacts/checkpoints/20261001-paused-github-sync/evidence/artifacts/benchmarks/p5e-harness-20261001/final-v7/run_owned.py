#!/usr/bin/env python3
"""Stable owned observer: wait actual child return code, not missing PID/last stdout.
Receipt/log never overwrite. Parent tool handle loss cannot break stdout pipe:
child stdout+stderr writes only to this owned regular file.
"""
import argparse,hashlib,json,os,pathlib,subprocess,time
p=argparse.ArgumentParser();p.add_argument('--log',type=pathlib.Path,required=True);p.add_argument('--receipt',type=pathlib.Path,required=True);p.add_argument('argv',nargs=argparse.REMAINDER);a=p.parse_args();argv=a.argv[1:] if a.argv and a.argv[0]=='--' else a.argv;assert argv and not a.log.exists() and not a.receipt.exists();a.log.parent.mkdir(parents=True,exist_ok=True)
started=time.time();progress=a.receipt.with_suffix('.started.json');assert not progress.exists()
with a.log.open('w') as stream:
 child=subprocess.Popen(argv,stdout=stream,stderr=subprocess.STDOUT)
 progress.write_text(json.dumps({'status':'owned_child_started_not_terminal','pid':child.pid,'started_epoch':started,'argv':argv,'cwd':os.getcwd()},indent=2)+'\n')
 actual_return_code=child.wait()
# Only actual wait completion writes terminal evidence. No FINAL-line/OSguess.
a.receipt.write_text(json.dumps({'status':'owned_wait_terminal','actual_return_code':actual_return_code,'pid':child.pid,'started_epoch':started,'finished_epoch':time.time(),'argv':argv,'cwd':os.getcwd(),'log':str(a.log),'log_sha256':hashlib.sha256(a.log.read_bytes()).hexdigest()},indent=2)+'\n');raise SystemExit(actual_return_code)
