# R30 observation-time correction

This appendix corrects the queued label in the R30 README and closeout published at commit `0be18dd597850880ad488e9e6978cfc92c6107ff`. The original files, archive and 140 source members remain byte-for-byte retained at that commit.

R30 closed at **2026-10-09 11:40:47 UTC**. The queued label described the last completed polling observation; it did not establish the actual job state at the cutoff. The subsequently available original log for LG run `37919399759`, job `113783411293`, starts runner setup at **11:40:06.0725587 UTC**. The source-admission command starts at **11:40:48.0899867 UTC**, after the cutoff. Therefore the retrospective actual state at the R30 cutoff was **in progress, setup**, with **no source-admission result, compiled controls result or native mixed measurement established by that cutoff**.

The later formatting failure belongs to R31 and is not backdated into R30. This timing correction changes no source, workflow, measurement, task status or acceptance requirement. R30 remains **0 newly completed original TODOs; 163 done and 29 remaining**.

The [complete original job log](LG-original-job.log) is retained here with SHA-256 `1a479309d49edb807418807e723ae2242937facb8fe5247b8d6fcae438ab7a97`; [official job metadata](LG-original-jobs.json) retains the original job identities. Later entries in those original files are evidence of later events, not evidence available at the R30 cutoff.
