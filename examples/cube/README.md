# Cube

The markdown catalog of [`../single-player`](../single-player) over a Cube source: sutura plans the
question from the catalog and Cube computes the metric over ClickHouse. Read the
[Cube page](../../docs/integrations/data-systems/cube.md).

| File                      | What it holds                                                            |
| ------------------------- | ------------------------------------------------------------------------ |
| `conf/base.yaml`          | The settings: the catalog, and the Cube source with its endpoint         |
| `compose.yaml`            | ClickHouse and Cube                                                      |
| `clickhouse/init.sql`     | The tables that Cube reads, loaded from the CSV files of `single-player` |
| `model/single_player.yml` | The Cube data model: one cube per table, one measure per catalog metric  |
| `setup.sh`                | Writes the secrets to `.env`, and the token that sutura sends to Cube    |

From this directory:

```sh
./setup.sh
docker compose up -d --wait
```

Cube listens on `127.0.0.1:4000`, the `endpoint` in `conf/base.yaml`. `setup.sh` needs `openssl` on
the host. It signs the token in `.local/cube-token` with the key that Cube verifies it with. The
token is valid for a day: run `./setup.sh` again after that. Then start sutura again: it reads the
token at startup.

Start sutura in a second terminal. `conf/base.yaml` reads the catalog from `examples/single-player`
relative to the working directory, and the token from `/run/sutura`:

```sh
docker run --pull always --rm --network host --user "$(id -u):$(id -g)" -w / \
  -v "$PWD/conf:/conf:ro" -v "$PWD/../single-player:/examples/single-player:ro" \
  -v "$PWD/.local:/run/sutura:ro" -e SUTURA_CONFIG_DIR=/conf \
  ghcr.io/telekom/sutura:${SUTURA_VERSION:-latest} serve
```

On Docker Desktop, turn on **Settings > Resources > Network > Enable host networking** first; with
it off, `127.0.0.1:8080` does not answer. See
[Single player](../../docs/examples/single-player.md).

Post the question. Its rows come from Cube, and the answer for June 2026 is `202121`:

```sh
curl -s -X POST http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}'
```
