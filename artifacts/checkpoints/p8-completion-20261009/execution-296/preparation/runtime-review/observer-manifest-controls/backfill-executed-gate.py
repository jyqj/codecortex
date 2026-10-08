assert r['observer_before'] == r['observer_after'] == r['observer_final'] and len(r['observer_before']['files']) == 9
assert r['observer_before']['source_commit'] == head, 'backfill observer manifest HEAD differs'
assert set(r['observer_before']['files']) == set(b.OBSERVER_FILES), 'backfill observer manifest inventory differs'
