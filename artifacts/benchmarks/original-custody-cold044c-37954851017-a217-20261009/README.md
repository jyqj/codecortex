# Original Actions artifact custody

First ten original ZIPs of cold run37954851017 attempt1: one build, five capacity observations, four successful rep0 shards (1k/5k/10k/50k). Original intake accepted4/150 shards and4/150 samples. The100k and remaining145 slices were pending at this snapshot; no aggregate acceptance. A root wrapper scalar-plan assertion failure and its schema-corrected read-only intake are both retained.

Each manifest identifies one complete original GitHub Actions ZIP. Chunks are exact byte ranges; they are not rebuilt ZIPs or replacement measurements. The preserved restore helper verifies all chunk SHA256/Git object IDs and the complete ZIP SHA256 before creating a new output.

From this directory, restore any listed artifact using:

```sh
python restore_original_zip.py manifests/ARTIFACT_ID.json --output /absolute/new/output.zip
```

Replace ARTIFACT_ID with an ID from catalog.json. An existing output is refused. Source, task definitions, historical studies and acceptance requirements are unchanged.
