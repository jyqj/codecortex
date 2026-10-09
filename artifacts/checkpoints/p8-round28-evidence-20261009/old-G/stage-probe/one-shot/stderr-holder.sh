#!/bin/sh
printf '%s\n' 'probe:started' >&2
while [ "$#" -gt 0 ]; do
 case "$1" in --output) shift; out="$1";; esac
 shift
done
printf '%s\n' 'probe:parsed' >&2
printf '%s\n' 'probe:before_fork' >&2
sleep 20 &
printf 'probe:after_fork descendant=%s\n' "$!" >&2
printf '{"passed":true}' > "$out/worker-summary.json"
printf '%s\n' 'probe:summary_written' >&2
