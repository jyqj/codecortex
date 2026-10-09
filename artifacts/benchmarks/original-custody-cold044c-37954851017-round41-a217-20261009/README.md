# Original Actions artifact custody

Five additional original ZIPs from unchanged044c/run37954851017/attempt1:100k repetitions2,3,9 plus capacity7,8. The root original validator batch already exited0 and advanced7to10/150 accepted coldshards. This preparation only preserves those complete ZIP bytes and existing six intake records; no validator/native rerun or partialaggregate. All prior custody remains in fixedparentc1b0d637; no task completion and no mixing with full193.

Each manifest identifies one complete original GitHub Actions ZIP. Chunks are exact byte ranges; they are not rebuilt ZIPs or replacement measurements. The preserved restore helper verifies all chunk SHA256/Git object IDs and the complete ZIP SHA256 before creating a new output.

From this directory, restore any listed artifact using:

```sh
python restore_original_zip.py manifests/ARTIFACT_ID.json --output /absolute/new/output.zip
```

Replace ARTIFACT_ID with an ID from catalog.json. An existing output is refused. Source, task definitions, historical studies and acceptance requirements are unchanged.
