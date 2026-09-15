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

The cell that drives it end to end is `crates/sutura-serve/tests/served/e2e.rs`'s
`the_wave_one_path_answers_a_verified_caller_under_the_shared_key`, invoked by `just e2e-datahub-bigquery`.

## What it does not demonstrate

Named rather than implied, because a green run read as proving more than it does is this page's
whole reason:

- **Execution AS the asking subject.** The source runs under one shared credential
  (`shared-service-user`), never a credential BigQuery resolves to the caller's own principal. That
  is the row [`where-identity-is-proven.md`](where-identity-is-proven.md) keeps **`unrun`**: no
  `iamcredentials` hop is built and no subject assertion is minted. The `SESSION_USER()` cell in
  `crates/sutura-exec-bigquery/tests/exchanged_identity.rs` stays `#[ignore]`d behind the
  maintainer's binding (issue #376 P2).
- **A real DataHub HTTP tier.** PR 1 answers issue #202's *recorded corpus* through an in-process
  fake server. The docker tier (`--datahub tier`) is the hosted job's, so local runs use the fake.
- **Anything about Postgres, Oracle or other sources.** BigQuery is the only adapter this showcase
  wires here.

## Running it locally, with your own project

Everything is a `just` task, and the BigQuery leg needs **your own open project**:

```text
just keycloak-tier start       # the real issuer: a realm, a client and two provisioned subjects
just e2e-datahub-bigquery      # boots the composed binary, loads the fixture into BigQuery, asks both subjects over HTTP
```

The task's cell FAILS, naming the missing one, when `GOOGLE_APPLICATION_CREDENTIALS` or
`SUTURA_BQ_DATASET` is unset - never a silent fallback to the in-process engine. (The credential
supplies its own billing project where it names one; `SUTURA_BQ_BILLING_PROJECT` is read only where
it does not.) The cell loads its own `orders`/`customers` tables into that dataset before asking and
drops them when it is done - not a table a developer has to seed by hand.
`examples/wave-one/base.yaml` is the settings template this cell's own shape maps onto, with every
value a reader owns named as a placeholder; the file's own header and the example README say which
is which.
