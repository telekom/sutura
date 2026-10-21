---
title: Slides
description: Slides from talks about sutura.
---

# Slides

<!-- dprint-ignore-start -->

<div class="grid cards" markdown>

-   __Introducing sutura__

    ---

    2026 · Why an agent should not write its own SQL, what sutura does instead, and two examples
    to run on a laptop.

    <a href="2026-introducing-sutura/">:octicons-arrow-right-24: Open the slides</a>

</div>

<!-- dprint-ignore-end -->

Each deck is a directory in
[`slides/`](https://github.com/telekom/sutura/tree/main/slides). `nix build .#slides` builds them.
To edit one, run `npm ci` and then `npm run dev` in its directory. A local `just docs` does not
include the decks.
