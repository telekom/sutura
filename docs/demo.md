# The local chat demo

`sutura` speaks HTTP, and a chat client can call it as a tool server. This page runs that: one
command brings the server up over `examples/single-player` beside a chat interface, with the browser
URL read out of the development tier's discovery file rather than written down anywhere.

**This page is a demonstration, not a deployment.** The chat client is UNGOVERNED: it chooses which
tool to call and how to phrase the answer, and no guarantee lives there. Every number it shows came
from a certified question the runtime answered, or from a refusal the runtime decided - the client
is a renderer and nothing more. And it is a SINGLE-USER demo: the deployment reads the example as
one shared service user, acknowledged by the operator, so it proves **neither caller identity nor
source impersonation**. [Where each identity claim is proven](where-identity-is-proven.md) is the
authority on what may be cited where.

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

Its limits, beside the claims:

- **Run end to end once, by hand, and by no gate.** On 2026-10-01, `just demo-mcp` on one
  developer machine brought the tier and the container up healthy. `/mcp` refused a call with no
  token, a malformed token and a token whose signature had been altered, each with `401`, and listed
  three tools for the minted one. Open WebUI's registry listed `server:mcp:sutura`, and one chat
  question went through Open WebUI's MCP client to `ask_metric`, which answered `202121` for June
  2026. Teardown left no container and stopped the tier. The model was a local deterministic stub
  that always picks that tool, so the run shows the plumbing, not a model choosing it. The
  demo-container workflow drives only `just demo`, and `just validate` runs this mode's launcher,
  supervisor, probe and minting helper against fakes.
- **One token, no renewal.** The tier's realm mints access tokens for an hour (Keycloak's default is
  five minutes, raised in `nix/keycloak-tier.nix`, whose start refuses a realm that mints less).
  Nothing refreshes the one token the launcher fetched, so after that hour the MCP connection is
  refused; run `just demo-mcp` again for a fresh one. Readiness latched its token check and stays
  healthy.
- **One subject, chosen by the launcher.** The chat client does not log in; every chat presents the
  same subject's token. It shows a real issuer's token verified on `/mcp`, not a person signing in.
- **Still a single shared source identity.** The verified caller does not change who reads the
  example - this is not source impersonation, and this demo is not a venue
  [where identity is proven](where-identity-is-proven.md) records.
- **The token travels in the container's environment**, the way the model key does, so anyone who
  can inspect the container can read it until it expires.

## Stop it

Ctrl-C removes the demo's container and its named volume. `examples/demo-chatinterface/start.sh` runs the
tier's own scoped teardown through `xtask dev-down`, so only this worktree's demo goes - the chat
client's stored state included. `just dev-down-demo` does the same from another shell.

## What the demo does not prove

Stated here rather than left to a reader, because an overstated control is itself the defect:

- **The chat client is ungoverned.** It has no sutura role. It chooses a tool, phrases a prompt and
  writes the answer text, and it can be wrong about all three - sutura's guarantees are untouched by
  that, because none of them live there.
- **A single-user demo proves no caller identity.** The deployment is `single-user` and reads the
  example as one shared service user, acknowledged by you. It neither knows nor shows who is asking,
  and it cannot be captioned as if it did.
- **It proves no source impersonation.** This demo uses a shared identity, so every question reads
  the example under the same operating-system identity. Leg 1 - the runtime knowing who is asking -
  is demonstrated elsewhere. The shipped BigQuery adapter has a per-subject path, but no served run
  has proven its source acceptance.
- **Development mode defaults rate limiting off.** It keeps the interface description available for
  the chat client's OpenAPI connection. Together with the unauthenticated single-user chat client,
  that makes loopback binding a boundary of this demo, not a pattern for a shared deployment.
- **The real walkthrough is not a gate, and this is not a release artifact.** `just validate` runs
  the hermetic fake-child behavior contract, including authenticated registry and operation-set
  checks, but it does not build the image or call a real model. `examples/demo-chatinterface/Dockerfile` is demo-only
  packaging around the shipped server binary. A demo that failed a gate would be disabled, and a
  disabled demo holds nothing.

For a configuration-only smoke check, run `just demo-check`. It validates endpoint transport policy,
required settings and acknowledgement without building an image, starting containers or contacting
the configured model. It does not prove that the model answers or that the chat client can call tools;
the full `just demo` lifecycle is required for that.

The generated OpenAPI document intentionally no longer publishes `GET /health`. This is a breaking
change to the published interface description: `/health` remains a runtime liveness route for probes,
but it is not a governed operation and therefore is not offered as a chat tool. Clients consuming the
OpenAPI document must stop expecting that path in the document.
