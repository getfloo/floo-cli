floo Feedback

Report bugs, friction, feature requests, or general feedback directly from
the CLI. Feedback is routed to the floo team in real-time.

## Usage

  floo feedback "your message here"
  floo feedback --category bug "deploys fail when Dockerfile is missing"
  floo feedback --category friction "env var sync requires a manual redeploy"
  floo feedback --category feature_request "add monorepo support"
  floo feedback --app my-app "this app crashes on cold start"

## Categories

  general          - general feedback (default)
  bug              - something is broken
  friction         - a rough edge or confusing workflow
  feature_request  - something you wish existed

## Agent Usage

  Agents should use --json mode. When --json is set, the source is recorded
  as "agent" instead of "cli" so the team can distinguish human vs agent
  feedback.

  floo feedback --json --category friction "deploy watch hangs after timeout"

## Context

  Use --context to attach up to 10,000 characters of extra detail (error
  output, steps to reproduce). The CLI validates the length before submitting:

  floo feedback --category bug "deploy fails" --context "error: no Dockerfile found"
