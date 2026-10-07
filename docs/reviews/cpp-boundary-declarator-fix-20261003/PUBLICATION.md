# Publication receipt

Fixed product: aa271e52b9c2aa52e05696af96ac52116963e57e.
Evidence/tasks/TODO commit: 20ad899 (same product tree).

Normal `git push -u origin fix/cpp-boundary-declarator` succeeded; upstream tracks
origin/fix/cpp-boundary-declarator. See origin-push.log.

Exactly one draft attempt:

```sh
gh pr create --repo jyqj/codecortex --base fix/python-declaration-kind --head fix/cpp-boundary-declarator --draft --title 'fix(parser): retain C/C++ declarator names after rejecting incompatible hints' --body-file /tmp/cpp-pr-body.md
```

The reviewed body is preserved as PR-BODY.md. GitHub returned:

```text
Post "https://api.github.com/graphql": Forbidden
```

Draft creation is blocked, not reported as successful. No alternative API,
credential changes, retries, merge or deployment were attempted. The pushed
branch is available for the parent thread to review; do not assume a PR exists.
