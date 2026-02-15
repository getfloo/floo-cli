# CLAUDE.md — Agent Instructions for getfloo/floo-cli

## What This Repo Is

This is the **public distribution repository** for the Floo CLI. It contains:
- Pre-built binaries (published via GitHub Releases)
- Install script (`install.sh`)
- User-facing documentation (README)
- CI workflow to sync releases from the private monorepo

**The CLI source code is NOT here.** It lives in the private `getfloo/floo` monorepo under `cli/` (Rust).

## Repository Contents

```
floo-cli/
├── README.md                    # Install instructions + quick start
├── install.sh                   # curl installer (detect OS/arch, download binary, verify checksum)
└── .github/
    └── workflows/
        └── sync-release.yml     # Triggered by private repo, publishes binaries as releases
```

## Release Flow

1. Tag `cli-v*` in private `getfloo/floo` repo
2. CI builds binaries for 4 platforms (macOS x86/arm, Linux x86/arm)
3. Cross-repo dispatch triggers `sync-release.yml` here
4. Binaries published as GitHub Releases on this public repo
5. Install script downloads from these releases

## Install Script

`install.sh` detects OS (macOS/Linux) and architecture (x86_64/arm64), downloads the correct binary from GitHub Releases, verifies the SHA256 checksum, and installs to `/usr/local/bin/floo`.

## Do NOT

- Add CLI source code here — it lives in `getfloo/floo`
- Modify the install script without testing on both macOS and Linux
- Create releases manually — they're automated from the private repo
