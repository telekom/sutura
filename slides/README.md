# Slides

Each talk deck lives in its own directory under `slides/` (currently `2026-introducing-sutura/`) and is published on the documentation site.

## Build

`nix build .#slides` builds the decks to a static site.

## Edit a deck

Run `npm ci` and then `npm run dev` in the deck's directory.

A local `just docs` does not include the decks.
