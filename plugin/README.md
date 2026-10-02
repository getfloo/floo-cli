# floo plugin

The plugin bundles the floo skill and MCP tools for deploying and operating apps.
Sign in to floo through your browser on first use to authorize the MCP connection
with OAuth.

## Claude Code

Add the marketplace and install the plugin:

```sh
claude plugin marketplace add getfloo/floo-cli
claude plugin install floo@floo
```

## Codex

Add the marketplace:

```sh
codex plugin marketplace add getfloo/floo-cli
```

Then open `/plugins` in Codex and install floo.

To connect the MCP server without the plugin:

```sh
codex mcp add floo --url https://api.getfloo.com/mcp
codex mcp login floo
```

## Cursor

Install floo from the Cursor marketplace. To connect the MCP server without the
plugin, add this entry to `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "floo": {
      "url": "https://api.getfloo.com/mcp"
    }
  }
}
```
