# Observation storage

45 original observation files (6,741,150 bytes) are stored losslessly in `raw-observations.tar.gz` (709,001 bytes). Two directly stored originals make 47 objects in the fixed SHA256/size manifest. Two independently generated canonical tar/gzip builds were byte-identical.

The original GitHub ZIP is stored as member `raw/pr144-p7-engineering-11496843573.zip`; its original SHA256 remains `7a1d199876e6a5c24593096d5406c2195e6c03ac645c05041ad15df5ba2ad2a1`. The 26 ZIP members are additionally retained as `raw/artifact/` files so the manifest binds all original log/snapshot/observation bytes individually. Full API responses, full decoded job log, exact expected/actual test lists and source comparisons are not shortened or rewritten. No raw mirror is committed.

`python3 verify_storage.py` checks the exact manifest and archive pins before parsing. It streams every member and verifies safe paths, types, canonical metadata, sizes and hashes; it rejects changed or unsafe optional disk mirrors and never extracts or executes payloads. Verification logic is reused unchanged from the independently reviewed P7-017 checkpoint; only six pin/size/count values change. A storage success does not imply acceptance of any task or erase the separately reported legacy CI failure.
