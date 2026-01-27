# Vendor Dependencies

This directory contains vendored external dependencies for the project.

## circomlib

- **Source**: https://github.com/iden3/circomlib
- **Commit**: `35e54ea21da3e8762557234298dbb553c175ea8d` (Update README.md #128)
- **Purpose**: Standard library of Circom circuits
- **Used by**: All circuits in `circuits/` directory
- **Components used**:
  - `bitify.circom` - Bit manipulation and conversion utilities
  - `compconstant.circom` - Constant comparison for range checks
  - `escalarmulfix.circom` - Fixed-base scalar multiplication for EdDSA
  - `escalarmulany.circom` - Variable-base scalar multiplication
  - `babyjub.circom` - BabyJubJub elliptic curve operations

### Why Vendored?

The circomlib is vendored (rather than using a git submodule) to ensure:
1. **Stability** - Circuit compilation remains consistent across environments
2. **Reproducibility** - Same circuit artifacts are generated
3. **No external dependencies** - Build works offline

### Updating

To update circomlib to a newer version:

```bash
cd vendor/circomlib
git fetch origin
git checkout <desired-commit-or-tag>
```

**Important**: After updating, regenerate all circuit artifacts and test thoroughly, as circomlib changes can affect circuit behavior and proof compatibility.
