# sutura identity test infrastructure

Test-grade Google infrastructure that makes issue #81's identity cells real. This is
**not** production infrastructure: it provisions disposable resources a developer (or
the `bigquery-acceptance` job) points at to prove the identity mechanism. It is managed
in-repo via Pulumi, and it holds **no real identifiers** - every project, pool and
principal name is a Pulumi config value supplied at run time.

## Why here, and why not in the hermetic sandbox

The nix test sandbox has no network, which is exactly what issue #82's postgres tier is
for (a data system reached over a socket). Identity is different: Google WIF and any
enterprise IdP are reached over a network, so this side is provisioned, and issue #105
owns the enterprise-IdP half. This project is the (a) Google half.

## What it provisions

- a dataset and a table with a grouping column;
- two service accounts, each granted a **disjoint** row set by a BigQuery row access
  policy - so the same question answered under each principal returns different rows.
  The isolation is BigQuery's row-level IAM; sutura's part is only that each job runs
  under its own bearer;
- a Google Workforce Identity Federation **pool + OIDC provider**, so a Google identity
  can be verified and exchanged at Google STS (the (a) token path). The exported
  `workforce_audience` is what a deployment declares as
  `sources.<alias>.workload_identity.audience`.

## Setup (one time, per developer or CI)

The real project/names live in your own `Pulumi.<stack>.yaml`, which is gitignored.
Nothing here requires (or permits) committing them.

```sh
cd test-infra/pulumi/google
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
# authenticate (ADC): gcloud auth application-default login, or a service account
cp Pulumi.example.yaml Pulumi.dev.yaml   # then edit with REAL values
pulumi stack init dev --copy-config-from=dev  # (adjust to your stack)
```

Then preview before applying, because this program is a scaffold you run, not a proof:

```sh
pulumi preview -s dev
pulumi up -s dev --yes
```

The two service-account **private keys are secret outputs** - capture them and store them
as environment secrets (e.g. GitHub `bq-test` secrets), never in the tree:

```sh
pulumi stack output --show-secrets -s dev principal_a_key > /dev/null
pulumi stack output --show-secrets -s dev principal_b_key > /dev/null
```

## What consumes the outputs

- the two keys become the `bq-test` environment's acceptance and second-service-account
  secrets, consumed by the `bigquery-acceptance` job;
- `principal_a_email` / `principal_b_email` and the row-grant mapping are what the
  two-principal acceptance cell asserts against;
- `workforce_audience` (and the pool provider) is the (a) end of a served
  `impersonation-at-source` source, once the exchanging broker is attached to one.

## Caveats

- **Unverified until `pulumi preview`.** This program is authored blind against the
  pulumi-gcp API; `preview` is the first thing that checks the resource shapes and the
  pinned `requirements.txt` version is the thing to re-check against the changelog first.
- The two-principal cell needs the table **populated** with at least one row per grouping
  value and the row access policy in place before it means anything.
- A row access policy is per-table; the grants here are the test-grade stand-in for a
  real entitlements mapping, and the exact row values are config (`principal_a_rows` /
  `principal_b_rows`), not logic.
