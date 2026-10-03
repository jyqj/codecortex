#!/usr/bin/env python3
"""Count existing public lane receipts without interpreting them as new metrics."""
import argparse
from collections import Counter
import json
from pathlib import Path


def audit(root,output):
    lanes=Counter();reasons=Counter();semantic=Counter();policies=Counter();rows=0;missing_receipts=0
    for commands in json.loads((root/'commands.json').read_text()):
        run=root/commands['key']
        for line in (run/'normalized.jsonl').read_text().splitlines():
            row=json.loads(line);raw=json.loads((run/row['raw_path']).read_text());r=raw.get('evidence_summary',{}).get('retrieval',{})
            rows+=1;semantic[str(r.get('policy',{}).get('semantic_state','unavailable'))]+=1
            policies[str(r.get('policy',{}).get('effective','unavailable'))]+=1
            if not r.get('lane_receipts'):missing_receipts+=1
            for lane in r.get('lane_receipts',[]):
                lanes[lane['lane_id']+'::'+lane['status']]+=1
                if lane['truncation_reason'] is not None:reasons[lane['lane_id']+'::'+lane['truncation_reason']]+=1
    result={'scope':'observed_group_pygo_public_full_run_only; counts of returned originating lane receipts, not current cache work',
        'observed_rows':rows,'missing_rows_no_lane_observation':888-rows,'observed_rows_without_lane_receipts':missing_receipts,
        'lane_status_counts':dict(sorted(lanes.items())),'lane_truncation_reason_counts':dict(sorted(reasons.items())),
        'semantic_state_counts':dict(semantic),'effective_policy_counts':dict(policies),'retrieval_calls':0}
    output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--run',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args();audit(a.run,a.output)
