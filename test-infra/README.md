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
- a Google Workload Identity Federation **pool + OIDC provider**, so a subject's own token
  can be exchanged at Google STS (the (a) token path). **Workload, not workforce** - the
  broker that performs sutura's exchange (`StsOverHttp`) sends an RFC 8693 `jwt` subject
  token, which is exactly the shape workload-pool OIDC providers accept (and workforce pools
  do not). The exported `workload_audience` is what a deployment declares as
  `sources.<alias>.workload_identity.audience`.

## Setup (one time, per developer or CI)

The real project/names live in your own `Pulumi.<stack>.yaml`, which is gitignored.
Nothing here requires (or permits) committing them.

```sh
# authenticate to Google once, from this repo: both logins in one container
just gl
cd test-infra/pulumi/google
cp Pulumi.example.yaml Pulumi.dev.yaml   # then edit with REAL values
pulumi stack init dev --copy-config-from=dev  # (adjust to your stack)
```

No venv, no `requirements.txt`: the Pulumi runtime (Python + `pulumi` + `pulumi-gcp`) is a
pixi environment (`infra`), so pixi owns the interpreter and everything in it - the same
reasoning the docs environment uses. `just infra-preview` and `just infra-up` (top-level
justfile) run Pulumi through it; anything you run with `pixi run -e infra` works too.

## Logging in: ADC vs the normal login

`just infra-gl` runs pixi's `gl` task, and that task does **both** gcloud
logins because doing one and forgetting the other is the failure that looks like a broken
adapter:

- **`gcloud auth login`** authorizes the **CLI** itself. It stores `credentials.db` in the
  gcloud config directory, and `gcloud` commands and `bq` use it.
- **`gcloud auth application-default login`** writes **ADC** (Application Default
  Credentials), `~/.config/gcloud/application_default_credentials.json`. That file is what a
  **client library** - the Pulumi GCP provider, Google client SDKs - reads automatically when
  there is no `GOOGLE_APPLICATION_CREDENTIALS` env var. **The Pulumi provider uses ADC**, so
  without this second login `pulumi preview` has no credentials to act under.

Both land in `~/.config/gcloud`, which is bind-mounted into the login container; nothing is
written into this repository. Everything that runs in this project afterwards - `pulumi
preview`, `pulumi up` - runs as the ADC principal, so it can only create the resources that
principal is allowed to create.

## No pulumi cloud: the local file backend

`preview`/`up` never talk to pulumi's cloud or a remote bucket. The state backbone is a
**`file://` URL** the justfile/workflow sets (`PULUMI_BACKEND_URL` → `<project>/.pulumi`,
gitignored) and stack-secrets are encrypted by a **`PULUMI_CONFIG_PASSPHRASE`** held in the
machine's `~/.config/sutura/env.sh` locally and as a `secret` in the `e2e-gcp` environment in
CI. Both are refused loudly when unset. `--local` as a CLI flag does not exist on the pinned
pulumi, which is why the backend travels as an environment variable rather than a flag.

CI mints a **per-run stack name** (`e2e-gcp-<run_id>-<run_attempt>`) so parallel runs never
touch the same local state.

Then preview before applying, because this program is a scaffold you run, not a proof. The stack
name comes from `SUTURA_PULUMI_STACK` (the machine env sets it to the developer's own) and is
passed to both the config step and pulumi, so a fresh shell needs no "active stack":

```sh
SUTURA_PULUMI_STACK=dev just infra-preview
SUTURA_PULUMI_STACK=dev just infra-up --yes
```

(`just infra-preview` / `just infra-up` configure the stack from the environment and run Pulumi
through the `infra` pixi env. The CLI is the nix-pinned `pulumi` and the SDK the pixi-locked one;
a gate in `config-from-env.sh` fails if the two ever disagree.)

The two service-account **private keys are secret outputs** - capture them and store them
as environment secrets (e.g. GitHub `bq-test` secrets), never in the tree:

```sh
pulumi stack output --show-secrets dev principal_a_key > /dev/null
pulumi stack output --show-secrets dev principal_b_key > /dev/null
```

## What consumes the outputs

- the two keys become the `bq-test` environment's acceptance and second-service-account
  secrets, consumed by the `bigquery-acceptance` job;
- `principal_a_email` / `principal_b_email` and the row-grant mapping are what the
  two-principal acceptance cell asserts against;
- `workload_audience` (and the pool provider) is the (a) end of a served
  `impersonation-at-source` source, once the exchanging broker is attached to one.

## The `e2e-gcp` GitHub environment

CI provisions this from `.github/workflows/e2e-gcp.yml` using a GitHub **environment
`e2e-gcp`** - no identifier is committed. Variables hold the resource names and secrets hold
the provider key:

| Kind | Names (`vars` / `secrets`) |
| --- | --- |
| `vars` | `E2E_GCP_PROJECT`, `E2E_GCP_REGION`, `E2E_GCP_DATASET`, `E2E_GCP_TABLE`, `E2E_GCP_GROUP_COLUMN`, `E2E_GCP_WORKLOAD_POOL_ID`, `E2E_GCP_WORKLOAD_PROVIDER_ID`, `E2E_GCP_WORKLOAD_ISSUER_URI`, `E2E_GCP_WORKLOAD_ALLOWED_AUDIENCES` |
| `secrets` | `E2E_GCP_ADMIN_KEY` (the provider's own service-account key) |

Since a fork's pull request cannot see an environment's secrets, the job skips there and runs
in-repo, the same `docs/adr/0017` rule as `bigquery-acceptance`. `config-from-env.sh` maps the
`SUTURA_GOOGLE_*` variables into stack config and refuses to run on anything missing, so a
half-configured environment fails loudly instead of previewing a broken stack.

## Caveats

- **Preview is green against pulumi_gcp 9.35.1** (`+14 to create`: the two API-enabling
  `Service`s, the two principals and keys, the dataset/table, the two `RowAccessPolicy`s,
  and the WIF pool/provider). What `preview` cannot check is the live endpoint: the first
  `up` against a real project is still the proof.
- The two-principal cell needs the table **populated** with at least one row per grouping
  value before it means anything.
- A row access policy is per-table; the grants here are the test-grade stand-in for a
  real entitlements mapping, and the exact row values are config (`principal_a_rows` /
  `principal_b_rows`), not logic. The one prerequisite outside the program is that the
  applying credential holds `serviceusage.services.enable`, since the program's own
  API-bootstrap `Service` resources turn the APIs on.
