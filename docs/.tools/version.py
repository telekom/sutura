"""Replace `{{ sutura_version }}` on every page with the workspace version in `Cargo.toml`.

`version-bump.yml` writes that one line at each release, so the pages follow without an edit.
"""

import re
from pathlib import Path

_FOUND = re.findall(
    r'^version = "([^"]+)"$', Path("Cargo.toml").read_text(), re.MULTILINE
)
if len(_FOUND) != 1:
    raise SystemExit(
        f"docs/.tools/version.py: expected one workspace version line in Cargo.toml, found {len(_FOUND)}"
    )
VERSION = _FOUND[0]


def on_page_markdown(markdown: str, **_: object) -> str:
    return markdown.replace("{{ sutura_version }}", VERSION)
