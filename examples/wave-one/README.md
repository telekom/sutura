# Wave one: DataHub metadata, a real issuer's token, BigQuery as the asking subject

The wave-one identity-aware E2E, as one worked path: a **`datahub` catalog** carries the certified
metric's definition, a **real Keycloak issuer**'s token says who is asking, and the deployment
answers **BigQuery as that asking subject** - over HTTP, on the composed `sutura-serve` binary.
This is a settings template and one worked question, not a directory of markdown: `base.yaml`, one
`question.yaml`, one expected refusal, and a README where every command is a `just` task.

## What this demonstrates, and what it does not

**Demonstrates** the wave's first three properties at once: a certified metric harvested from
[DataHub metadata](../multi-player/), asked through HTTP `/v1/query` over a real issuer's verified
token, and answered from BigQuery **as the asking subject** - the audit record's `subject` names
who asked, never the deployment. The served cell that drives this end to end is
`crates/sutura-serve/tests/served/e2e.rs`'s `the_wave_one_path_answers_as_the_asking_subject`.

**Does not demonstrate** the exchanged-identity half, and it says so rather than hiding it: that the
deployment obtained, per subject, a service-account credential BigQuery resolves to a different
principal than the deployment's own. That is exactly the row
[`docs/where-identity-is-proven.md`](../../docs/where-identity-is-proven.md) keeps **`unrun`** -
no `iamcredentials` hop is built and the `bq-test` environment holds no
`SUTURA_BQ_PRINCIPAL_{A,B}_ASSERTION`. The `SESSION_USER()` cell beside the served one stays
`#[ignore]`d behind the maintainer's binding (issue #376 P2) and says so. It also does not
demonstrate a **real DataHub HTTP tier**: PR 1 answers the recorded corpus through an in-process
fake server; the docker tier is the hosted job's (`--datahub tier`, PR 2).

## The settings

`base.yaml` is the template - a `datahub` catalog (the five keys issue #202's served PR adds, with
the two path fields it requires for every kind), an impersonating `bigquery` source on your own
service-account key, `multi-user` identity, the issuer's `inbound` block, and the exchanged-
credential cache **explicitly off** (`security.credential_cache.enabled: false`) so each ask is one
fresh exchange, never a cached credential masking a broken leg.

Every value in `<>` is yours: your DataHub endpoint and personal-access-token file, the
structured-property name your deployment chose for the certified metric's document ([ADR 0016](../../docs/adr/0016-what-datahub-can-carry.md) is what that document must carry), your
issuer's audience and JWKS, and a BigQuery billing project, dataset and key shaped like
`bq-test`.

## The question

`question.yaml` asks the certified `revenue` metric, over exactly its anchor range:

```yaml
metric: revenue
grain: month
range:
  start: 2026-06-01
  end: 2026-07-01
```

Over the wire this is the JSON body `served/e2e.rs`'s cell posts to `POST /v1/query`:

```bash
curl -s -X POST http://127.0.0.1:<port>/v1/query \
  -H "Authorization: Bearer <ascii_a_token>" \
  -H "Content-Type: application/json" \
  -d '{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}'
```

A `200` means it was answered, with `outcome: "answer"`, a `provenance` naming the DataHub-sourced
definition, and an `executed_as` naming the `impersonation-at-source` posture. The request and its
answer are the value that cell asserts, not a transcript kept in step by hand.

## The refusal

Asking a question this catalog does not certify - presented with a VALID principal token, so what
is refused is the question - comes back as a **typed refusal with its reason**, never `200`.
`refusal.json` is the expected shape (`docs/serving.md`'s "an explicit status, the stable `code` it
always carried, and a sentence saying what to change"); the exact status/code is what the served
cell pins against the running binary, so treat the numbers here as the shape, not a verified value.
A request with no credential at all is refused `401`, `code: "unauthorized"`, before the question is
looked at.

## Running it

Everything is a `just` task, in the order the path needs it:

```text
just keycloak-tier start          # the real issuer: a realm, a client and two provisioned subjects
just dev-up                       # the data systems this worktree's compose profile serves
just e2e-datahub-bigquery         # the wave-one E2E: boots the composed binary and asks both subjects
```

`just e2e-datahub-bigquery` starts the Keycloak tier itself if it is not up, but `just dev-up` (or
`just dev-up-datahub`, for the real docker tier the hosted job pays for) is what a local run needs
for the data-system side, and the BigQuery leg needs **your own open project shaped like `bq-test`**

- `GOOGLE_APPLICATION_CREDENTIALS`, `SUTURA_BQ_DATASET`, `SUTURA_BQ_BILLING_PROJECT` and
  `SUTURA_BQ_WORKLOAD_AUDIENCE` on the environment. The task's cell fails **closed** on their absence,
  naming the missing one; it does not silently degrade to answering as the deployment, because that
  is the exact lie the wave exists to refuse. `just keycloak-tier stop` tears the issuer back down.

Read what is missing from the run as carefully as what is in it: the run you can do locally today
proves DataHub→HTTP with a real issuer's token→BigQuery **answered**, and the refusal; it does not
prove the exchanged identity, which stays the maintainer's binding in
[`docs/where-identity-is-proven.md`](../../docs/where-identity-is-proven.md).
