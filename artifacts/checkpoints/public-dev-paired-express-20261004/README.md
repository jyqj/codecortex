# Fixed same-input Express public DEV paired verification

Pre-ranking freeze: baseline 88f2cf099c8b81f3acef485fd5ac9b01c63ce790,
candidate 37dd042eaa1209a86e0cafdcd92ae77e036e76f5; candidate production src/Cargo/lock
matches 90858afae647a513537bf118932a7ba5020ee98b. Native/compat use exact original
PR91 admission, no PYGO taxonomy. Prior DEV score exposure is disclosed in plan.
Each arm schedules 210 native + 177 compat = 387. Repetitions 3, warmup 0,
seed 20261003, top_k 10, timeout 30000, original hybrid MCP adapter/config.
Two isolated checkouts/targets/binaries/input roots; cc-eval creates independent
throwaway source/cache/index per suite and requires prepare + Ready before queries.
First offline builds failed missing reqwest cache; diagnostics retained. Official
locked Cargo fetch succeeded; fresh isolated builds in progress. No rankings yet.

Only this prefix is owned. Central tasks, old evidence, product, gold, scorer,
normalizer, budgets/providers/default knobs remain unchanged. No holdout/private
export, excluded tests, merge/deploy or new CI success claim. Old1671 remains all
Partial/qualityFAIL; fixed public DEV 301 native/256 compat/280 correlated groups
is not 600 or clean holdout. Clean holdout = 0.
