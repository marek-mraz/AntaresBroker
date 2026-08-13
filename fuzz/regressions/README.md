# Fuzz-crash regression corpus

One directory per fuzz target. When the weekly `fuzz.yml` run finds a
crash, commit the crashing input here (from the uploaded
`fuzz/artifacts/<target>/` file) in the SAME commit as the fix. The daily
`strict.yml` `fuzz-regression` job replays every file with `-runs=0`, so a
fixed crash can never silently return.

Empty directories are normal — they mean no crash has been found yet.
