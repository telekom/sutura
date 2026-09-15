# Wave one: the identity-aware E2E showcase

This page is the worked, runnable version of the wave-one path - **DataHub metadata → a real
issuer's token → BigQuery as the asking subject** - alongside `examples/wave-one/README.md`, the
drill-down. It is a showcase, not a guarantee: read the exchange half below, because it is the one
thing a local run does *not* prove.

## What it demonstrates

One certified metric, asked over HTTP, answered from BigQuery **as the asking subject**:

1. **DataHub carries the definition.** A `catalog.kind: datahub` entry (issue #202's served form)
   harvests the certified `revenue` metric from the structured property its deployment names
   ([ADR 0016](adr/0016-what-datahub-can-carry.md)).
2. **A real issuer says who is asking.** The deployment declares `security.inbound` against the
   provisioned Keycloak realm - `mode: direct`, the realm's own audience, JWKS and `RS256` - so a
   caller's token is verified against a real provider's signature, not the mock that "answers yes by
   construction".
3. **BigQuery executes as the subject.** The `impersonation-at-source` source presents the subject's
   exchanged credential, and the served audit record's `subject` names who asked - principal A and
   principal B produce **different** subjects.
4. **The refusal is typed.** An uncertified question comes back as a refusal that carries its reason;
   a request with no credential is refused `401` before the question is looked at.

The cell that drives it end to end is `crates/sutura-serve/tests/served/e2e.rs`'s
`the_wave_one_path_answers_as_the_asking_subject`, invoked by `just e2e-datahub-bigquery`.

## What it does not demonstrate

Named rather than implied, because a green run read as proving more than it does is this page's
whole reason:

- **The exchanged identity** - that the deployment obtained, *per subject*, a service-account
  credential BigQuery resolves to a different principal than itself. That is the row
  [`where-identity-is-proven.md`](where-identity-is-proven.md) keeps **`unrun`**: no
  `iamcredentials` hop is built and no subject assertion is minted. The `SESSION_USER()` cell beside
  the served one stays `#[ignore]`d behind the maintainer's binding (issue #376 P2). A local green
  run answers *as the subject only in the sense the exchange half is `unrun`* - read that row.
- **A real DataHub HTTP tier.** PR 1 answers issue #202's *recorded corpus* through an in-process
  fake server. The docker tier (`--datahub tier`) is the hosted job's, so local runs use the fake.
- **Anything about Postgres, Oracle or other sources.** BigQuery is the only adapter that can carry
  a per-subject credential at all.

## Running it locally, with your own project

Everything is a `just` task, and the BigQuery leg needs **your own open project** shaped like the
`bq-test` environment - the showcase is not self-contained, and it says so rather than implying it
is:

```text
just dev-up                    # the data systems this worktree's compose profile serves
just keycloak-tier start       # the real issuer: a realm, a client and two provisioned subjects
just e2e-datahub-bigquery      # boots the composed binary and asks both subjects over HTTP
```

The task's cell fails **closed** when a leg is unconfigured - missing `GOOGLE_APPLICATION_CREDENTIALS`,
`SUTURA_BQ_DATASET`, `SUTURA_BQ_BILLING_PROJECT` or `SUTURA_BQ_WORKLOAD_AUDIENCE` stops it naming the
missing one, never a silent fallback to answering as the deployment. `examples/wave-one/base.yaml` is
the settings template with every value a reader owns named as a placeholder; the file's own header and
the example README say which is which.
