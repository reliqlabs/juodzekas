# Git Submodules

This project uses git submodules for external dependencies.

## circomlib

The project depends on [circomlib](https://github.com/iden3/circomlib) for standard Circom circuit components.

### Initial Clone

When cloning this repository, you need to initialize submodules:

```bash
git clone <repository-url>
cd juodzekas
git submodule update --init --recursive
```

Or clone with submodules in one command:

```bash
git clone --recursive <repository-url>
```

### Updating Submodules

To update to the latest version of circomlib:

```bash
git submodule update --remote vendor/circomlib
```

### Current Version

- **circomlib**: Pinned to commit `35e54ea21da3e8762557234298dbb553c175ea8d`
- See `vendor/README.md` for details on what components are used

### Important Notes

⚠️ **After updating circomlib**, you must:
1. Regenerate circuit artifacts in `circuits/artifacts/`
2. Clear the ZK proof key cache in `packages/zk-shuffle/tests/cache/`
3. Run the integration tests to ensure compatibility

The circuits and proofs are sensitive to circomlib changes!
