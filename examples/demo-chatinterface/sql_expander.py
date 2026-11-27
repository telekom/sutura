"""
title: SQL Query Expander
author: sutura contributors
version: 0.2.0
description: >
  Outlet: collapses verify-sql fenced blocks into a <details> element so the
  model's mandatory reproducibility block stays out of the way until the reader
  wants it.  Works with DOMPurify because <details>/<summary>/<pre>/<code> are
  all in the allow-list.

  Inlet: injects the sutura tool-server id into every chat request so the model
  does not need the user to enable it manually in the UI.
"""

from __future__ import annotations

import html
import re
from typing import Any

# Pattern: ```verify-sql ... ``` (fenced, possibly with trailing whitespace after the tag)
_VERIFY_SQL = re.compile(
    r"```verify-sql[^\n]*\n(.*?)```",
    re.DOTALL,
)


def _collapse(match: re.Match[str]) -> str:
    code = html.escape(match.group(1).rstrip())
    return (
        "<details>"
        "<summary>verify-sql</summary>"
        f"<pre><code>{code}</code></pre>"
        "</details>"
    )


class Filter:
    def inlet(self, body: dict[str, Any], __user: dict | None = None) -> dict[str, Any]:
        """Inject the sutura tool-server so the model can call it without the user enabling it."""
        tool_ids: list[str] = body.get("tool_ids") or []
        if "server:sutura" not in tool_ids:
            tool_ids = [*tool_ids, "server:sutura"]
        body["tool_ids"] = tool_ids
        return body

    def outlet(self, body: dict[str, Any], __user: dict | None = None) -> dict[str, Any]:
        """Collapse verify-sql blocks into <details> so they fold by default."""
        messages: list[dict[str, Any]] = body.get("messages") or []
        for message in messages:
            if message.get("role") != "assistant":
                continue
            content = message.get("content")
            if isinstance(content, str):
                message["content"] = _VERIFY_SQL.sub(_collapse, content)
            elif isinstance(content, list):
                for part in content:
                    if isinstance(part, dict) and part.get("type") == "text":
                        part["text"] = _VERIFY_SQL.sub(_collapse, part["text"])
        return body
