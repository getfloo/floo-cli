# langchain-floo

Load floo tools into a Python LangChain agent. The tools use the permissions of the floo API key supplied by the application.

Install from this repository:

```bash
python -m pip install 'langchain-floo @ git+https://github.com/getfloo/floo-cli.git#subdirectory=python/langchain-floo'
```

Set `FLOO_KEY` in your application environment. To give an inventory assistant access to two read tools:

```python
import asyncio

from langchain_floo import load_floo_tools


async def main():
    tools = await load_floo_tools(tool_names=["list_orgs", "list_apps"])
    print([tool.name for tool in tools])


asyncio.run(main())
```

Pass the returned tools to LangChain's `create_agent`. Omit `tool_names` to load all tools exposed by floo to that credential. To supply the key from your application's secret store, pass `api_key` to `load_floo_tools`.

[floo MCP guide](https://getfloo.com/docs/guides/mcp#langchain)
