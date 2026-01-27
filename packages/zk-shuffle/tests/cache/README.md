# ZK Proof Key Cache

This directory stores cached proving and verifying keys for the ZK circuits to avoid expensive regeneration.

## Files

- `shuffle.pk` (~170 MB) - Shuffle circuit proving key (uncompressed)
- `shuffle.vk` (~14 KB) - Shuffle circuit verifying key
- `reveal.pk` (~2.5 MB) - Reveal circuit proving key (uncompressed)
- `reveal.vk` (~1 KB) - Reveal circuit verifying key

## Notes

- Keys are serialized in uncompressed format for faster deserialization (30-60s vs several minutes)
- Loading cached keys takes ~17s vs ~4 minutes for regeneration
- Keys are deterministic based on the circuit R1CS/WASM files
- `.gitignore` excludes these files as they're too large for git and can be regenerated

## Regeneration

If you delete these files, they will be automatically regenerated on the next test run.
This takes approximately 4 minutes but only needs to be done once.
