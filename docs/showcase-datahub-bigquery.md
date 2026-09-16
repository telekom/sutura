# Wave one: the identity-aware E2E showcase

This page is the worked, runnable version of the wave-one path - **DataHub metadata → a real
issuer's token → a real BigQuery project** - alongside `examples/wave-one/README.md`, the
drill-down. It is a showcase, not a guarantee: read the exchange half below, because it is the one
thing a local run does *not* prove.

## What it demonstrates

One certified metric, asked over HTTP, answered from a REAL BigQuery project under one shared
credential:

1. **DataHub carries the definition.** A `catalog.kind: datahub` entry (issue #202's served form)
   harvests the certified `revenue` metric from the structured property its deployment names
   ([ADR 0016](adr/0016-what-datahub-can-carry.md)).
2. **A real issuer says who is asking.** The deployment declares `security.inbound` against the
   provisioned Keycloak realm - `mode: direct`, the realm's own audience, JWKS and `RS256` - so a
   caller's token is verified against a real provider's signature, not the mock that "answers yes by
   construction".
3. **BigQuery executes the question.** A `bigquery` source under `posture: shared-service-user`
   loads its own `orders`/`customers` fixture tables into a real dataset and answers the certified
   metric from them - one shared credential for whoever asks. The served audit record's `subject`
   still names who asked, per token, even though the SOURCE executes as one identity for both.
4. **The refusal is typed.** An uncertified question comes back as a refusal that carries its reason,
   pinned against `examples/wave-one/refusal.json`'s `status`/`code`; a request with no credential is
   refused `401` before the question is looked at.
5. **The same question over `/mcp`.** The agent surface mounts the SAME serving surface behind the SAME
   leg-1 gate, so the certified question asked over the streamable-HTTP agent surface returns the SAME
   row total and the same `verified` subject tied to its own token as the HTTP ask - the byte-for-byte
   join over one deployment. A verified caller is advertised the `ask_metric` tool, an unverified
   `/mcp` request is refused `401` before the transport, and an uncertified question is a typed refusal inside the tool result
   (`outcome: "refusal"`, `reason.code: "metric_unknown"` - the same `code` the HTTP refusal pins; the
   REST `status` field has no tool-result counterpart).

   The exact call, as a model chooses it:

   ```json
   {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ask_metric","arguments":{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}}}
   ```

   The `arguments` object is exactly `examples/wave-one/question.yaml` - the same question text both
   transports answer.

The cell that drives it end to end is `crates/sutura-cli/tests/served/e2e.rs`'s
`the_wave_one_path_answers_a_verified_caller_under_the_shared_key`, invoked by `just e2e-datahub-bigquery`.

## What it does not demonstrate

Named rather than implied, because a green run read as proving more than it does is this page's
whole reason:

- **Execution AS the asking subject.** The source runs under one shared credential
  (`shared-service-user`), never a credential BigQuery resolves to the caller's own principal. That
  is the row [`where-identity-is-proven.md`](where-identity-is-proven.md) keeps **`wired`**: no
  `iamcredentials` hop is built and no subject assertion is minted. The `SESSION_USER()` cell in
  `crates/sutura-exec-bigquery/tests/exchanged_identity.rs` stays `#[ignore]`d behind the
  maintainer's binding (issue #376 P2).
- **Anything about Postgres, Oracle or other sources.** BigQuery is the only adapter this showcase
  wires here.

The two DataHub modes and what each proves are the next section; the missing piece above (execution
AS the subject) is the one a green run of either mode still does not demonstrate.

## The two DataHub modes

`just e2e-datahub-bigquery` takes one flag, `--datahub fake|tier`, defaulting to `fake`.

- **`--datahub fake`** (the default locally, no docker) answers issue #202's *recorded corpus*
  through an in-process HTTP fake (`sutura_catalog_datahub::test_support::FakeServer`). It proves
  DataHub→HTTP on a real issuer's token→a real BigQuery project, and the refusal. Its DataHub leg
  is the corpus, not the platform.
- **`--datahub tier`** (the hosted job's) starts the REAL docker DataHub platform
  (`xtask dev-up --with datahub` - five containers), provisions the certified `revenue` metric
  under the deployment's own structured property ([ADR 0016](adr/0016-what-datahub-can-carry.md))
  exactly as `datahub-acceptance` does, has the tier mint its own personal-access token offline with
  its own signing key (headless GMS exposes no `/auth/*` surface - see `dev/src/mint.rs`; the PAT is
  never committed, it is written to a generated `token_file` at run time), and points the served
  binary's HTTP `AspectReader` at the platform. Same one cell, same three asks, same anchors -
  fail-not-skip on every step.

Both modes run the identical cell through the identical settings builder; the flag only decides
which `endpoint`/`token_file` the deployment reads from. A docker tier is still not a production
DataHub: single node, generated secrets, auth enabled for the minted PAT alone.

## Running it locally, with your own project

Everything is a `just` task, and the BigQuery leg needs **your own open project**:

```text
just keycloak-tier start              # the real issuer: a realm, a client and two provisioned subjects
just e2e-datahub-bigquery             # the wave: the recorded corpus over the fake (no docker)
just e2e-datahub-bigquery -- --datahub tier   # the wave over the REAL docker DataHub tier (needs docker)
```

In `--datahub tier` mode the task (or the nix app `nix run .#e2e-datahub-bigquery -- --datahub tier`)
brings the DataHub platform up with `xtask dev-up --with datahub`, provisions the metric, mints the
PAT, and leaves the platform running - `just dev-down` tears it down.

The task's cell FAILS, naming the missing one, when `GOOGLE_APPLICATION_CREDENTIALS` or
`SUTURA_BQ_DATASET` is unset - never a silent fallback to the in-process engine. (The credential
supplies its own billing project where it names one; `SUTURA_BQ_BILLING_PROJECT` is read only where
it does not.) The cell loads its own `orders`/`customers` tables into that dataset before asking and
drops them when it is done - not a table a developer has to seed by hand.
`examples/wave-one/base.yaml` is the settings template this cell's own shape maps onto, with every
value a reader owns named as a placeholder; the file's own header and the example README say which
is which.
