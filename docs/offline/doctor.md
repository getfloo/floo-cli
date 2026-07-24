floo Doctor

Doctor commands return one verdict plus the evidence behind it. Every command
supports --json and exits non-zero when it detects an issue.

## Managed Redis data-plane health

  floo doctor managed-services --app <name>
  floo doctor managed-services --app <name> --json

floo executes one short-lived read/write canary against every ready dev, prod,
and preview Redis resource each minute. The doctor reports throttling, auth or
permission rejection, unreachable providers, command rejection, configuration
errors, and missing or stale observations.

Each issue includes service, environment, stable status and reason, observation
time, and remediation. It never includes Redis credentials, raw provider
responses, or customer keys.

For request-rate or quota throttling, reduce Redis traffic and contact
team@getfloo.com to raise managed capacity.

## Accounts-mode drift

  floo doctor accounts --app <name>
  floo doctor accounts --app <name> --json

The accounts doctor compares requested access config with the gateway state
currently serving it. It exits non-zero when the drift list is non-empty.

Full guide: https://getfloo.com/docs/cli/doctor
