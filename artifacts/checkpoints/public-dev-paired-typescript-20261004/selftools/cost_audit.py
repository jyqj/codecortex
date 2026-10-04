#!/usr/bin/env python3
"""Report observed cost leaves per suite/arm; no configuration change or inferred missing zero."""
import json
from collections import defaultdict
from paired import OUT,SHAS,dump
from analyze import rows
result={}
def numeric_leaves(x,path=''):
 if isinstance(x,dict):
  for k,v in x.items():
   if k not in ['schema_version','scan_cap']:yield from numeric_leaves(v,path+'.'+k if path else k)
 elif isinstance(x,list):
  for item in x:
   name=item.get('stage') if isinstance(item,dict) else None
   yield from numeric_leaves(item,path+('/'+name if name else '[]'))
 elif isinstance(x,(int,float)) and not isinstance(x,bool):yield path,x
for arm in SHAS:
 suites={};total=defaultdict(list)
 for entry in json.loads((OUT/'plan.json').read_bytes())['suite_entries']:
  p=OUT/'runs'/arm/entry['key']/'costs.jsonl';leaves=defaultdict(list);rs=rows(p) if p.exists() else []
  for r in rs:
   if r.get('work') is not None:
    for key,value in numeric_leaves(r['work']):leaves[key].append(value);total[key].append(value)
  suites[entry['key']]={'scheduled':entry['scheduled_rows'],'observed_cost_rows':len(rs),'work_available':sum(r.get('work') is not None for r in rs),'leaves':{k:{'observed_values':len(v),'sum':sum(v),'mean':sum(v)/len(v)} for k,v in sorted(leaves.items())}}
 result[arm]={'suites':suites,'leaves':{k:{'observed_values':len(v),'sum':sum(v),'mean':sum(v)/len(v)} for k,v in sorted(total.items())}}
dump(OUT/'retrieval-cost-audit.json',{'interpretation':'originating-work leaves only, costs.jsonl untouched; schema_version/scan_cap are identity/budget not summed; overlapping nested counters are never added to each other; missing stays missing; no money/token zero or performance causal claim','arms':result})
