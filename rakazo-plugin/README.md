# floo for Rakazo

Give a Rakazo bot access to floo to build, deploy, and update a hosted app. Your floo sign-in determines which org and apps the bot can access.

## Connect a bot

1. Open this folder on GitHub and copy its URL. For a test branch, press **Y** first to pin the URL to the commit.
2. In Rakazo, open **Integrations → + → From GitHub**, paste the folder URL and choose **Preview**. Expect one server named `floo`, zero skills, and zero skipped parts.
3. Choose **Connect**, then **Sign in** on the floo server. Review and approve access for the floo account you want to use. Select your bot and choose **Use in this bot**.
4. Ask the bot to call `list_apps` and confirm the expected org before creating an app. An empty list is valid for a new org.

## Test a weekly support report

Use a floo test org with deployment credits. These prompts use fictional ticket counts. Keep the report behind the starter's team sign-in. Each verification targets dev.

**First report**

> Our Monday support report counts tickets by queue. For November 23, 2026, Billing had 8 and Setup had 5. Build a floo-hosted app called Weekly Support Report with the week, both queue counts, and their total on the home page. Use a new app in the test org. Deploy to dev, wait for this change to go live, and return the app ID, deploy ID, and dev URL.

The bot can use `plan_deploy` and `create_app` to create the hosted project. It can inspect the starter with `list_files` and `read_file`, then commit the report with `write_files`. Match the commit to its deploy with `list_deploys` and wait with `get_deploy`. Open the returned URL as a member of the app. Check the date, both queues, and **13** on the rendered page. Share the URL with another existing app member to check team access.

**Next report**

> Update that same app for November 30, 2026. Billing had 7 tickets and Setup had 9. Keep the previous week visible. Deploy to dev and return the app ID, new deploy ID, and URL when this commit is live.

Open the original URL again. The November 30 total must be **16**, and the November 23 total must still be **13**. Record the plugin commit, successful OAuth return, bot tool result, app ID, both deploy IDs, URL, and the two observed totals for marketplace review. Keep credentials out of the test record.

## Connect an existing app with OpenAPI

For a bot that only needs to call your app's routes, use **Integrations → + → OpenAPI**. Enter the full HTTPS URL of the app's OpenAPI document, such as the app's dev URL followed by `/openapi.json` or `/api/openapi.json`. The spec must describe the intended environment. For routes protected by an app API key, select **Bearer token** and enter an app API key scoped to those routes. Choose **Verify and add**, then ask the bot to call a read-only operation and check its result.

Through the floo MCP connection, the bot can also discover operations with `search_app_api` and call them with `read_app_api` or `write_app_api`. Supply the app ID and environment on each call. [Call your app](https://getfloo.com/docs/guides/call-your-app) · [App API keys](https://getfloo.com/docs/guides/app-api-keys) · [Rakazo connections](https://rakazo.com/docs/integrations/)
