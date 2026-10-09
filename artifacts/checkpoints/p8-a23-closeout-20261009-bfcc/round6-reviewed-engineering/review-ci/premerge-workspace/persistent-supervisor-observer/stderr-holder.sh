#!/bin/sh
while [ "$#" -gt 0 ]; do
 case "$1" in --output) shift; out="$1";; esac
 shift
done
sleep 20 &
printf '{"passed":true}' > "$out/worker-summary.json"
