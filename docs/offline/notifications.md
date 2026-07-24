Email Notifications - control which emails floo sends you

floo emails you about things that happen to your apps. You choose which
categories land in your inbox; account and security messages always send.

## List your settings

  floo notifications list
  floo notifications list --json     (machine-readable, for agents)

Shows every category, whether it is on or off, and what it covers.

## Turn a category on or off

  floo notifications set deploy_success on    Email me on every successful deploy
  floo notifications set deploy_success off   Stop those (this is the default)
  floo notifications set billing off          Stop spend-cap warning emails

## Categories

  Run `floo notifications list` to see the current categories and their state.
  deploy_success is OFF by default (it is the noisy one); the rest are ON.

## Notes

  Preferences are per-user and account-wide - they apply to your inbox, not to a
  single app. Always-send emails (invites, verification, security approvals,
  and destructive-action warnings) are not configurable.
