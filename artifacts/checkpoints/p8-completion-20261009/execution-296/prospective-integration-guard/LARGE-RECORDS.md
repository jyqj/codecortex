# Large original preparation records

The two large JSON files listed in `large-original-records-delivery.json` are delivered as exact members of `large-original-records.tar.gz`. Their uncompressed original bytes, sizes and SHA256 values match the existing `preservation-manifest.json`. The original plain files are retained locally; they are excluded only from the Git delivery payload to keep individual tool uploads bounded.

The archive contains only `preparation/linked-base-files.json` and `preparation/object-and-space-inventory.json`. Verify the archive SHA256 and each member size/SHA256 against the delivery record, then restore them into a separate directory. No binary, Git object store or complete checkout is included. These are preparation records for the explicitly scoped prospective file projection; they do not certify actual main, CI, scale or any TODO.
