# getfloo

Run the [floo](https://getfloo.com) CLI with `npx`. floo deploys and hosts apps built with AI.

```bash
npx getfloo deploy
```

Or install it once:

```bash
npm install -g getfloo
floo --help
```

The first run downloads the signed floo binary for your machine from the
[floo-cli releases](https://github.com/getfloo/floo-cli/releases), checks its
SHA-256 checksum and RSA signature, and keeps it at `~/.floo/bin/floo`. Later
runs start that binary directly. `floo update` moves it to the newest release.

Docs: https://getfloo.com/docs. Agents start at https://getfloo.com/agents.md.
