---
name: floo
description: Find floo documentation and CLI commands when building, deploying, or operating an app on floo.
user-invocable: false
---

# floo

Read the relevant canonical documentation before changing a floo app:

- [Quickstart](https://getfloo.com/docs/introduction) for setup and a first deploy.
- [Agent setup](https://getfloo.com/docs/guides/agent-setup) for authority, verification, and operational guidance.
- [Documentation index](https://getfloo.com/docs/llms.txt) for configuration, services, authentication, and other topics.

Use `floo docs --json` to find topic URLs and `floo docs <topic> --json`
to select one. These commands return links; fetch and read the linked page.

Use `floo commands --json` to discover commands and `floo <command> --help`
for syntax supported by the installed version. Use `--json` for structured output.

After `floo auth login`, follow any terms acceptance instruction. Run
`floo auth accept-terms --yes` only after the human has agreed to the
[Terms of Service](https://getfloo.com/legal/terms) and
[Privacy Policy](https://getfloo.com/legal/privacy). Never infer agreement.
