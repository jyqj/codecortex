assert build['source_before'] == build['source_after'] == plan['source'] and build['observer_before'] == build['observer_after']
assert build['observer_before']['source_commit'] == head, 'runtime observer manifest HEAD differs'
assert set(build['observer_before']['files']) == set(owner.OBSERVER_FILES), 'runtime observer manifest inventory differs'
