# The local chat demo

`sutura` speaks HTTP, and a chat client can call it as a tool server. This page runs that: one
command brings the server up over `examples/single-player` beside a chat interface, with the browser
URL read out of the development tier's discovery file rather than written down anywhere.

**This page is a demonstration, not a deployment.** The chat client is UNGOVERNED: it chooses which
tool to call and how to phrase the answer, and no guarantee lives there. Every number it shows came
from a certified question the runtime answered, or from a refusal the runtime decided - the client
is a renderer and nothing more. And it is a SINGLE-USER demo: the deployment reads the example as
one shared service user, acknowledged by the operator, so it proves **neither caller identity nor
source impersonation**. [Inbound identity](integrations/identity.md) describes what sutura
verifies about a caller.

## What it brings up

`just demo` builds and starts ONE container holding two supervised processes:

| Process         | What it is                                                                                   |
| --------------- | -------------------------------------------------------------------------------------------- |
| `sutura serve`  | The shipped HTTP server, over `examples/single-player` - the same binary a release publishes |
| The chat client | Open WebUI v0.11.3, pointed at a language model you configure                                |

The client is registered against the server through Open WebUI's **native OpenAPI connection** -
`type: openapi`, the server's loopback URL, `path: openapi.json`, a bearer `auth_type`, and
`config.enable` - so there is no plugin to maintain, and the tools it lists are exactly the
operations the served document already describes.

`examples/demo-chatinterface/run.sh` is the supervisor. It starts both children and, if either exits, stops the other and
fails the container - so a demo that lost half of itself is never reported healthy. `examples/demo-chatinterface/healthcheck.py`
is the probe, and it checks the server, its two operations and the client. The port is published
ephemerally and bound to loopback, so two worktrees can run the demo at once and nothing off this
machine can reach it.

## Configure the model

Three values and one acknowledgement, all from the environment. `examples/demo-chatinterface/start.sh` validates them
before it builds or starts anything, and never prints a value: a missing or malformed setting is
reported by name.

| Variable                     | Required              | What it is                                                            |
| ---------------------------- | --------------------- | --------------------------------------------------------------------- |
| `SUTURA_DEMO_MODEL_ENDPOINT` | yes                   | An OpenAI-compatible base URL - hosted, or a local server             |
| `SUTURA_DEMO_MODEL`          | yes                   | The model id that endpoint serves                                     |
| `SUTURA_DEMO_MODEL_API_KEY`  | for a hosted endpoint | The key; a model on loopback or `host.docker.internal` needs none     |
| `SUTURA_DEMO_ACKNOWLEDGE`    | yes                   | Your own sentence for why one identity reading one example is correct |

**Hosted.** Point the endpoint at your provider and set the key.

```bash
export SUTURA_DEMO_MODEL_ENDPOINT="https://api.example.com/v1"
export SUTURA_DEMO_MODEL="a-tool-calling-model"
export SUTURA_DEMO_MODEL_API_KEY="..."   # never printed, never written into this repository
export SUTURA_DEMO_ACKNOWLEDGE="one person, one host, one local example"
just demo
```

**Local.** Run a server that speaks the OpenAI API - Ollama's `/v1` does - and point the demo at the
name a container reaches it by. `host.docker.internal` is the host, from inside the container.

```bash
SUTURA_DEMO_MODEL_ENDPOINT="http://host.docker.internal:11434/v1" \
SUTURA_DEMO_MODEL="qwen3" \
SUTURA_DEMO_ACKNOWLEDGE="one person, one host, one local example" \
just demo
```

No key is needed for a loopback or `host.docker.internal` endpoint, and that is the only difference
between the two shapes. The client needs a model that can call tools.

## Open it, and try the two tools

`just demo` starts the tier and prints the browser URL from `just dev-endpoint demo` - the only way
to learn the port, because it is allocated rather than chosen:

```
demo: open http://127.0.0.1:49xxx in a browser
```

The client lists exactly two tools, from the same declaration the agent surface uses:
`describe_catalog` and `ask_metric`. Both go to the certified runtime; the model decides when to
call one and writes the prose around the result.

**A certified answer.** Ask something the catalog defines.

> Which regions did we bill recurring revenue in during June 2026?

The model calls `ask_metric`, and the runtime answers from `recurring_revenue` grouped by region,
under the definition digest it reports beside the rows - the same certified figure
`examples/single-player` pins, which sums to 202121 minor units (2021.21) for the month. The client
shows a number; the provenance came from the runtime.

**A refusal.** Ask for something the catalog does not define.

> What is the customer lifetime value for June 2026?

The model calls `ask_metric`, and the runtime refuses - `metric_unknown`, with a sentence naming the
metric - because a question the catalog cannot express is a RESULT, not an error, and the client can
only render it. Repeating the question does not change the answer.

Some models answer from their own knowledge instead of calling a tool. That is the client being
ungoverned: if it did not call `ask_metric`, no certified number was involved.

## Over MCP, behind a real issuer

`just demo-mcp` is the same demo with the chat client on Open WebUI's native **MCP** connection
instead of OpenAPI, and the server's `/mcp` mounted behind inbound verification against the nix
Keycloak tier (`just keycloak-tier`). It takes the same four variables and adds, in order:

1. It starts the Keycloak tier - and stops it on exit if this run started it.
2. `examples/demo-chatinterface/keycloak_token.py` asks the realm for an access token as its first
   subject, over the tier's own CA, and fetches the realm's key set. Nothing it reads is printed.
3. The container's deployment declares `security.inbound` in `direct` mode over that issuer, its
   audience and that key set, with no deployment token; the MCP connection presents the minted token.
4. Readiness requires `/mcp` to refuse a call that carries no token with `401`, on every probe, and
   to initialize and list tools for the minted token once.

The server never contacts the issuer: it verifies against the key set it was handed, which is why
the tier's loopback `https://` issuer verifies inside a container whose loopback is its own.

Good to know:

- **One token, no renewal.** The tier's realm mints access tokens for an hour (Keycloak's default is
  five minutes, raised in `nix/keycloak-tier.nix`, whose start refuses a realm that mints less).
  Nothing refreshes the one token the launcher fetched, so after that hour the MCP connection is
  refused. Run `just demo-mcp` again for a fresh one.
- **One subject, chosen by the launcher.** The chat client does not log in. Every chat presents the
  same subject's token.
- **The example is read as a shared service user.** The verified caller does not change who reads
  the example.
- **The token travels in the container's environment**, the way the model key does, so anyone who
  can inspect the container can read it until it expires.

## Stop it

Ctrl-C removes the demo's container and its named volume. `examples/demo-chatinterface/start.sh` runs the
tier's own scoped teardown through `xtask dev-down`, so only this worktree's demo goes - the chat
client's stored state included. `just dev-down-demo` does the same from another shell.

## What the demo is

- **The chat client is not part of sutura.** It chooses a tool, phrases a prompt and writes the
  answer text. sutura's guarantees do not depend on it.
- **The deployment is `single-user`.** It reads the example as one shared service user, which you
  acknowledge.
- **Development mode defaults rate limiting off.** It keeps the interface description available for
  the chat client's OpenAPI connection, so loopback binding is the boundary of this demo. It is not
  a pattern for a shared deployment.
- **The Dockerfile is demo-only packaging** around the server binary
  (`examples/demo-chatinterface/Dockerfile`).

For a configuration-only smoke check, run `just demo-check`. It validates endpoint transport policy,
required settings and acknowledgement without building an image, starting containers or contacting
the configured model.

The generated OpenAPI document does not list `GET /health`. `/health` is a liveness route for
probes, not a governed operation, so the chat client does not get it as a tool.
