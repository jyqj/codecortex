from pathlib import Path
import json,hashlib,re,subprocess
R=Path(__file__).resolve().parent;E=R/'source/artifacts/checkpoints/capability-snapshot-optimization-20261003'
checks=json.loads((E/'checks.json').read_text())
counts=[]
for c in checks:
 s=(E/c['log']).read_text()
 counts.append({'name':c['name'],'reported_exit':c['exit_code'],'test_results':re.findall(r'test result: (.*)',s)})
cases=[]
for p in sorted((E/'ab').glob('*/receipt.json')):
 receipt=json.loads(p.read_text());poll=json.loads((p.parent/'polls.json').read_text());cases.append({'case':p.parent.name,'receipt':receipt,'poll_type':type(poll).__name__,'poll_records':len(poll)})
# Read all remaining bounded author text evidence (source/patch/log/JSON), retained identity in source-identity.json.
for p in E.rglob('*'):
 if p.is_file():p.read_text()
ci=(R/'source/.github/workflows/ci.yml').read_text()
report={'author_checks':counts,'author_check_commands':len(checks),'author_test_executions_expected':46,'independent_pass_credit':0,'author_AB_cases':cases,'ci_default_current_source':True,'ci_semantic_product_build':True,'ci_semantic_http_command':bool(re.search(r'--features\s+semantic-http',ci)),'ci_current_oracle_cfg_requires_semantic':True,'ci_path_note':'default workspace compiles actual capability_status and cc-db; optional semantic stdio build compiles current production; migrated integration oracle is cfg(semantic), not executed by default workspace test; semantic-http targeted oracles not present in CI YAML','evidence_phase_note':'source-evidence.json and POINT-IN-TIME-REVIEW are historical design artifacts; FINAL-VERIFICATION and delivery are final phase','no_author_AB_reexecution':True}
(R/'author-evidence-audit.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'commands':len(checks),'results':counts,'cases':len(cases),'http_CI':report['ci_semantic_http_command']},indent=2))
