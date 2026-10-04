"""Read-only original raw/scorer replay using the pinned candidate lib build."""
import json,subprocess,sys
from pathlib import Path
out=Path(__file__).resolve().parent;base=Path(sys.argv[1]);record=json.loads((out/'micro-candidate/receipt.json').read_text());cmd=record['link'];cmd[2]=str(out/'replay_driver.rs');cmd[cmd.index('-o')+1]=str(base/'replay-driver');subprocess.run(cmd,check=True)
p=subprocess.run([str(base/'replay-driver'),str(base),str(out/'micro-candidate-receiver/results.json')],check=True,stdout=subprocess.PIPE)
(out/'scorer-replay.json').write_bytes(p.stdout);print(p.stdout.decode(),end='')
