# Single player

A markdown catalog over local CSV files, served for one user. Read the walkthrough in
[`docs/examples/single-player.md`](../../docs/examples/single-player.md), or on the
[documentation site](https://telekom.github.io/sutura/latest/examples/single-player/).

This demo needs Docker and local files. It needs no cloud account or cloud administrator role.
An enterprise deployment uses source accounts and read grants that operators create in advance.

Quick start, from the repository root:

```bash
docker compose -f examples/single-player/compose.yaml up
curl http://127.0.0.1:8080/health
```
