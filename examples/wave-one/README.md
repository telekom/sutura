# Wave one: DataHub metadata, a real issuer's token, a real BigQuery project

The wave-one identity-aware E2E, as one worked path: a **`datahub` catalog** carries the certified
metric's definition, a **real Keycloak issuer**'s token says who is asking, and the deployment
answers from a **real BigQuery project**, under one shared credential - over HTTP, on the composed
`sutura-serve` binary. This is a settings template and one worked question, not a directory of
markdown: `base.yaml`, one `question.yaml`, one expected refusal, and a README where every command
is a `just` task.

## What this demonstrates, and what it does not

**Demonstrates** the wave's first three properties at once: a certified metric harvested from
[DataHub metadata](../multi-player/), asked through HTTP `/v1/query` over a real issuer's verified
token, and answered from a REAL BigQuery project under `posture: shared-service-user` - one
credential for whoever asks. The audit record's `subject` still names who asked, per token, even
though the source itself executes as one shared identity. The served cell that drives this end to
end is `crates/sutura-serve/tests/served/e2e.rs`'s `the_wave_one_path_answers_a_verified_caller_under_the_shared_key`.

**Does not demonstrate** execution AS the asking subject, and it says so rather than hiding it: that
the deployment obtained, per subject, a service-account credential BigQuery resolves to a different
principal than the deployment's own. That is exactly the row
[`docs/where-identity-is-proven.md`](../../docs/where-identity-is-proven.md) keeps **`unrun`** - no
`iamcredentials` hop is built and no per-subject assertion is minted. The `SESSION_USER()` cell in
`crates/sutura-exec-bigquery/tests/exchanged_identity.rs` stays `#[ignore]`d behind the maintainer's
binding (issue #376 P2) and says so. It also does not demonstrate a **real DataHub HTTP tier**: PR 1
answers the recorded corpus through an in-process fake server; the docker tier is the hosted job's
(`--datahub tier`, PR 2).

## The settings

`base.yaml` is the template - a `datahub` catalog (the five keys issue #202's served PR adds, with
the two path fields it requires for every kind), a `bigquery` source on your own service-account
key, `single-user` identity, the issuer's `inbound` block, and `posture: shared-service-user`. This
is the shape `served/e2e.rs`'s runnable cell maps onto; it is not the exchanged-identity shape,
which stays the maintainer's binding.

Every value in `<>` is yours: your DataHub endpoint and personal-access-token file, the
structured-property name your deployment chose for the certified metric's document ([ADR 0016](../../docs/adr/0016-what-datahub-can-carry.md) is what that document must carry), your
issuer's audience and JWKS, and a BigQuery billing project, dataset and key of your own.

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
  -H "Authorization: Bearer <subject-a-token>" \
  -H "Content-Type: application/json" \
  -d '{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}'
```

A `200` means it was answered, with `outcome: "answer"`, a `provenance` naming the DataHub-sourced
definition, and an `executed_as` naming the `shared-service-user` posture. The request and its
answer are the value that cell asserts, not a transcript kept in step by hand.

## The refusal

Asking a question this catalog does not certify - presented with a VALID principal token, so what
is refused is the question - comes back as a **typed refusal with its reason**, never `200`.
`refusal.json` is the expected shape (`docs/serving.md`'s "an explicit status, the stable `code` it
always carried, and a sentence saying what to change"), and `served/e2e.rs`'s cell pins its
`status`/`code` fields against the running binary rather than merely checking they are present.
A request with no credential at all is refused `401`, `code: "unauthorized"`, before the question is
looked at.

## Running it

Everything is a `just` task, in the order the path needs it:

```text
just keycloak-tier start          # the real issuer: a realm, a client and two provisioned subjects
just dev-up                       # the data systems this worktree's compose profile serves
just e2e-datahub-bigquery         # the wave-one E2E: loads the fixture into BigQuery and asks both subjects
```

`just e2e-datahub-bigquery` starts the Keycloak tier itself if it is not up, and it loads its own
`orders`/`customers` tables into your BigQuery dataset before asking (dropping them when it is
done) - the BigQuery leg needs **your own open project**:

- `GOOGLE_APPLICATION_CREDENTIALS` and `SUTURA_BQ_DATASET` on the environment.
  `SUTURA_BQ_BILLING_PROJECT` is read only where the credential names no project of its own. The
  task's cell fails **closed** on either's absence, naming the missing one; it does not silently
  degrade to answering from the in-process engine, because that is the exact lie the wave exists to
  refuse. `just keycloak-tier stop` tears the issuer back down.

Read what is missing from the run as carefully as what is in it: the run you can do locally today
proves DataHub→HTTP with a real issuer's token→a real BigQuery project **answered**, and the
refusal; it does not prove execution AS the asking subject, which stays the maintainer's binding in
[`docs/where-identity-is-proven.md`](../../docs/where-identity-is-proven.md).
