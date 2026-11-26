"""LangChain tools for the floo cloud."""

import os
from collections.abc import Collection

from langchain_core.tools import BaseTool
from langchain_mcp_adapters.client import MultiServerMCPClient


async def load_floo_tools(
    *, api_key: str | None = None, tool_names: Collection[str] | None = None
) -> list[BaseTool]:
    """Discover floo tools for this credential, optionally selecting by exact name."""
    key = api_key if api_key is not None else os.environ.get("FLOO_KEY")
    if not key or not key.strip():
        raise ValueError("Pass api_key or set FLOO_KEY to a floo API key.")
    if isinstance(tool_names, str):
        raise TypeError("Pass a collection of tool names, such as ['list_apps'].")

    base_url = os.environ.get("FLOO_API_URL", "https://api.getfloo.com").rstrip("/")
    client = MultiServerMCPClient(
        {
            "floo": {
                "transport": "http",
                "url": f"{base_url}/mcp",
                "headers": {"Authorization": f"Bearer {key.strip()}"},
            }
        }
    )
    tools = await client.get_tools()
    if tool_names is None:
        return tools

    selected = set(tool_names)
    missing = selected.difference(tool.name for tool in tools)
    if missing:
        raise ValueError(f"Unavailable floo tools: {', '.join(sorted(missing))}")
    return [tool for tool in tools if tool.name in selected]


__all__ = ["load_floo_tools"]
