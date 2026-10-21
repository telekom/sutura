"""Copy the built meetup deck into the site, so it is published beside the pages.

`nix build .#checks.x86_64-linux.slides` makes the deck. The docs workflow names its output in
`SUTURA_SLIDES_DIST`; without it, as in a local `just docs`, the site has no deck and this hook
does nothing.
"""

import os
import shutil
from pathlib import Path


def on_post_build(config: dict, **_: object) -> None:
    dist = os.environ.get("SUTURA_SLIDES_DIST")
    if not dist:
        return
    target = Path(config["site_dir"]) / "slides"
    shutil.copytree(Path(dist), target, dirs_exist_ok=True)
    # The store is read-only and `copytree` copies the modes, which stops the next `--clean`.
    for path in (target, *target.rglob("*")):
        path.chmod(path.stat().st_mode | 0o200)
