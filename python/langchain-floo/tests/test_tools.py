import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from langchain_floo import load_floo_tools


class TestToolSelection(unittest.IsolatedAsyncioTestCase):
    async def test_explicit_selection_does_not_expose_unrequested_tools(self):
        read = SimpleNamespace(name="list_apps")
        write = SimpleNamespace(name="create_app")
        with (
            patch.dict("os.environ", {"FLOO_API_URL": "https://example.test/"}),
            patch("langchain_floo.MultiServerMCPClient") as client,
        ):
            client.return_value.get_tools = AsyncMock(return_value=[read, write])
            tools = await load_floo_tools(api_key="test", tool_names=["list_apps"])
            self.assertEqual(tools, [read])
            self.assertEqual(
                client.call_args.args[0]["floo"]["url"], "https://example.test/mcp"
            )
            with self.assertRaisesRegex(ValueError, "missing_tool"):
                await load_floo_tools(api_key="test", tool_names=["missing_tool"])
