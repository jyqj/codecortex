#!/usr/bin/env python3
"""只读逐项要求/证据盘点；只写本独占审计目录，不改变任务状态。"""
import collections
import datetime
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
TASKS = ROOT / 'docs/roadmap/code-index-v2/tasks.json'

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def capture():
    plan = json.loads(TASKS.read_text())
    evidence_paths = set()
    for task in plan['tasks']:
        for ev in task.get('evidence', []):
            evidence_paths.update(ev.get('artifacts', []))
            for key in ['commands_receipt', 'baseline_commands_receipt']:
                if isinstance(ev.get(key), str):
                    evidence_paths.add(ev[key])
    files = {}
    for name in sorted(evidence_paths):
        p = ROOT / name
        row = {'exists': p.exists(), 'is_file': p.is_file()}
        if p.is_file():
            row['bytes'] = p.stat().st_size
            if row['bytes'] <= 2_000_000:
                row['sha256'] = sha(p)
                if p.suffix == '.json':
                    try:
                        value = json.loads(p.read_text())
                        if isinstance(value, dict):
                            row['receipt_status'] = value.get('status')
                            row['recorded_head'] = value.get('head', value.get('target_sha'))
                            row['source_digest'] = value.get('source_digest_sha256')
                    except (ValueError, UnicodeError):
                        row['parse_error'] = True
            else:
                row['hash_scope'] = 'not_read_large_artifact'
        files[name] = row
    requirements = []
    for t in plan['tasks']:
        row = {k: t.get(k) for k in ['id', 'phase', 'batch', 'title', 'status', 'scope',
              'depends_on', 'conditional_dependencies', 'steps', 'deliverables', 'acceptance',
              'validations', 'conditional', 'required_for', 'rollback', 'implementation_notes']}
        row['recorded_evidence'] = t.get('evidence', [])
        row['evidence_paths'] = sorted({name for ev in t.get('evidence', []) for name in ev.get('artifacts', [])})
        row['missing_evidence_paths'] = [name for name in row['evidence_paths'] if not files[name]['exists']]
        row['proof_classification'] = ('historical_done_requires_current_behavior_revalidation'
                                       if t['status'] == 'done' else 'incomplete_current_task')
        row['closure_requirement'] = ('explicit_conditional_decision_and_scope_impact_or_authorized_live_proof'
                                      if t.get('conditional') else 'implemented_and_current_behavior_verified')
        requirements.append(row)
    return {
        'observed_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'worktree': subprocess.check_output(['git', 'status', '--short', '--branch'], cwd=ROOT, text=True),
        'tasks_path': str(TASKS.relative_to(ROOT)), 'tasks_sha256': sha(TASKS),
        'status_counts': dict(collections.Counter(t['status'] for t in plan['tasks'])),
        'phase_counts': {phase: dict(collections.Counter(t['status'] for t in plan['tasks'] if t['phase'] == phase))
                         for phase in plan['phase_order']},
        'milestones': {m: {'ids': [t['id'] for t in plan['tasks'] if m in t['required_for']],
                           'statuses': dict(collections.Counter(t['status'] for t in plan['tasks'] if m in t['required_for']))}
                       for m in sorted({m for t in plan['tasks'] for m in t['required_for']})},
        'evidence_files': files, 'requirements': requirements,
        'limitations': ['File existence/hash and receipt status are not behavior proof.',
                        'No full Rust test rerun in this inventory.',
                        'Historical done is not current release certification.',
                        'No paid/live provider authorization inferred from all-todolist objective.',
                        'Optional decision completion is not unimplemented feature completion.'],
    }

if __name__ == '__main__':
    target = OUT / (sys.argv[1] if len(sys.argv) > 1 else 'round-01.json')
    if target.exists():
        raise SystemExit('Refuse to overwrite an existing audit snapshot')
    data = capture()
    target.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'snapshot': str(target.relative_to(ROOT)), 'task_count': len(data['requirements']),
                     'counts': data['status_counts'], 'evidence_file_count': len(data['evidence_files']),
                     'missing_files': [k for k,v in data['evidence_files'].items() if not v['exists']]},
                    ensure_ascii=False, indent=2))
