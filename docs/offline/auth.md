App Auth - Add User Authentication to Your App

floo manages user authentication for your deployed apps. Set
access_mode = "accounts" and floo's gateway puts a hosted sign-in
flow in front of your app, validates each user's session, and injects
identity headers into every request before it reaches your code.

You write no auth code. No login pages. No OAuth flow. Your app reads
X-Floo-User-Email from the request headers - that is the entire
integration.

## Quickstart

  1. Set the access mode in floo.app.toml:

       [app]
       name = "my-app"
       access_mode = "accounts"

  2. Deploy:

       git push origin main

  3. Read identity headers in your app code (every authenticated
     request has them):

       X-Floo-User-Email: jane@acme.com
       X-Floo-User-Id:    01HQK4...
       X-Floo-User-Name:  Jane Doe
       X-Floo-User-Role:  member

That is the entire setup. There is no [auth] section to configure,
no callback URLs to register, no client ID to provision, no token
exchange to implement.

## What you get

  - Hosted WorkOS sign-in for floo identities, including Google and
    other configured providers
  - Session cookie (__floo_session) validated on every request,
    rolled forward as users stay active, revoked on sign-out
  - Identity headers (X-Floo-User-Email/Id/Name/Role) injected on
    every authenticated request
  - GET /__floo/me        - signed-in user JSON {user_id,email,name,role};
                            401 if no session, 403 HTML if access-denied
  - POST|DELETE /__floo/logout - clears floo session and 302s to login.
                            Lands at app root / after re-auth. Other methods
                            (including GET) return 405; SameSite=Lax + GET
                            would have allowed cross-origin drive-by sign-out.
                            Does NOT log the user out of WorkOS - federated
                            SLO is not yet supported.
  - Explicit per-app member list with member/admin tenant roles
  - Immediate session and app-key revocation when app access is removed

## Invite app users

Accounts-mode apps are invite-only. Invite a floo identity from the
dashboard Access tab or the CLI:

  floo apps invite teammate@acme.com --role member --app my-app
  floo apps invites --app my-app

List responses include `next_cursor` when more results exist. Continue
with `--cursor NEXT_CURSOR` for invitations or members.

The recipient creates or reuses the matching global floo account, then
accepts the app membership. App access never grants dashboard, deploy,
organization, billing, or platform API-key authority.

Invitation URLs are secret-shaped and redacted by default. When you need
to hand off the URL yourself, add global `--reveal-secrets`; otherwise the
provider email is the delivery path.

Manage pending invitations:

  floo apps invite-resend INVITE_ID --app my-app
  floo apps invite-revoke INVITE_ID --app my-app

Resending rotates the old link. List and manage active app members:

  floo apps members --app my-app
  floo apps member-role MEMBERSHIP_ID admin --app my-app
  floo apps member-remove MEMBERSHIP_ID --app my-app

Organization membership does not imply app membership. Invite operators
to every app they should use. Changing a role revokes sessions carrying
the old role; removing a member revokes their app sessions and keys.

## Access Modes

  public    - no auth, anyone can access (default)
  password  - shared password for simple protection (Pay as you go+)
  accounts  - per-user auth, gateway-managed (Pay as you go+)

Enterprise SSO (SAML/OIDC) is a sales-assisted setup, not a self-serve
access_mode value - email sales@getfloo.com if your team needs it.

Per-environment overrides work too:

  [environments.dev]
  access_mode = "public"

For password-protected apps, set access_mode = "password" in
floo.app.toml. The platform generates the shared password on the
next deploy. Retrieve it with:

  floo apps password my-app

## Reading the user in your code

Stack-specific examples are in the build journey guides:

  floo docs rails    - Ruby on Rails
  floo docs nextjs   - Next.js (App Router)
  floo docs fastapi  - FastAPI
  floo docs django   - Django
  floo docs express  - Express (Node.js)

The pattern is the same in every stack: read X-Floo-User-Email
from request headers in a middleware or controller hook.

## Local development

For accounts-mode apps, `floo dev --fixture-user EMAIL` starts a
small in-process proxy in front of each service that injects the
same X-Floo-User-* headers the gateway adds in production:

  floo dev --app my-app --fixture-user you@example.com

The output shows two URLs per service - the raw service URL and an
auth-proxied URL. Hit the auth-proxied URL when you want to test
signed-in flows; your app sees the four identity headers exactly
as it would behind the gateway. Hit the raw URL for quick checks
or unauthenticated paths.

Optional flags (with defaults):

  --fixture-id ID      default: dev-fixture-<email-localpart>
  --fixture-name NAME  default: the email
  --fixture-role ROLE  default: member

The proxy only runs when access_mode = "accounts". For one-off
curl testing or scripts, send the headers yourself:

  curl -H "X-Floo-User-Email: you@example.com" \
       -H "X-Floo-User-Id: dev-user-1" \
       http://localhost:3000/dashboard

Full docs: https://getfloo.com/docs/guides/app-auth
