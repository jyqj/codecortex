# Adapted Werkzeug header utilities

`source/src/requests/utils.py` contains `parse_list_header`, `parse_dict_header`
and `unquote_header_value` adapted from Werkzeug. The original upstream notices
`From mitsuhiko/werkzeug (used with permission).` remain verbatim in that file.
No assertion of a separate private permission agreement is made here.

Copyright (c) 2010 by the Werkzeug Team, see AUTHORS for more details.

The complete, original BSD copyright notice, redistribution conditions,
non-endorsement condition and disclaimer are preserved in
[`werkzeug-0.6.2/LICENSE`](werkzeug-0.6.2/LICENSE), alongside original
[`AUTHORS`](werkzeug-0.6.2/AUTHORS). Preserve those files with this snapshot.
Requests' primary LICENSE/NOTICE retain their original bytes; they do not replace
the adapted utilities' BSD notice. No contributor names are used as endorsements.

Primary evidence:

- Werkzeug 0.6.2 fixed Git commit `d902d2c05a2cb3d7d1f5414fe9c678273f5ffa05`:
  [LICENSE](https://github.com/pallets/werkzeug/blob/d902d2c05a2cb3d7d1f5414fe9c678273f5ffa05/LICENSE),
  [AUTHORS](https://github.com/pallets/werkzeug/blob/d902d2c05a2cb3d7d1f5414fe9c678273f5ffa05/AUTHORS),
  [http.py](https://github.com/pallets/werkzeug/blob/d902d2c05a2cb3d7d1f5414fe9c678273f5ffa05/werkzeug/http.py).
- Requests' first permission-notice introduction:
  [9966017a4976339c76b834eab8a10b4dfba474f1](https://github.com/psf/requests/commit/9966017a4976339c76b834eab8a10b4dfba474f1),
  dated 2011-10-23, “Add new utilities from werkzeug”. Original utility file
  and introduction patch are retained as license-lineage evidence outside the
  evaluator source root.
  The historical Requests LICENSE (ISC), NOTICE and AUTHORS are also preserved
  beside that historical source evidence; these do not replace the modern
  pinned Requests Apache LICENSE.
- `werkzeug-0.6.2/lineage.json` binds source-file SHA256 and normalized AST hashes.
  All three function implementations match across Werkzeug 0.6.2, Requests'
  introduction and the locked Requests SHA after removing docs/type annotations
  and normalizing annotated assignment. Algorithm statements are unchanged by
  that normalization. This demonstrates an already licensed implementation,
  rather than claiming to know the precise historical copied release.

The three utilities remain excluded from question/gold admission, preserving all
previous decisions. Retaining their licensed source bytes in the full unmodified
`utils.py` is now supported by this explicit lineage and attribution record.
Independent reviewer acceptance remains pending.
