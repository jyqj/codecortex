from pathlib import Path
p=Path('review-pr/pr178-ci-observation/artifact-observer.py')
exec(compile(p.read_text().split('ROOT.mkdir(exist_ok=True)\n')[0],str(p),'exec'))
prefix=OBS/'raw-review'/'lifecycle-11601537047-v4'
row=native(['/Library/Developer/CommandLineTools/usr/bin/python3','review-pr/pr178-ci-observation/reviewer-scripts/lifecycle-independent-review-v4.py','11601537047','ce8fa5b4e19321a1ff3dc2d738ffa5347b405ccd2e515063223157b564d5c157','6c0c6e21150216d4b46b5872047c6d6cf6a31d4a8f122bf5a07b70fde45c376c'],prefix.with_suffix('.stdout'),prefix.with_suffix('.stderr'),prefix.with_suffix('.command.json'),300)
print(json.dumps(row))
raise SystemExit(row['exit_code'])
