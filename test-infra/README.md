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
just infra-gl
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
**`file://` URL** the justfile sets (`PULUMI_BACKEND_URL` → `<project>/.pulumi`, gitignored)
and stack-secrets are encrypted by a **`PULUMI_CONFIG_PASSPHRASE`** held in the machine's
`~/.config/sutura/env.sh`. Both are refused loudly when unset. `--local` as a CLI flag does
not exist on the pinned pulumi, which is why the backend travels as an environment variable
rather than a flag.

**Provisioning is an operator step, not CI.** The stack is applied once on a developer machine
(`just infra-up`, as the operator's gcloud ADC); no CI job provisions, previews or destroys
stack state. CI only *consumes* the outputs - see "The `bq-test` GitHub environment" below.

Then preview before applying, because this program is a scaffold you run, not a proof. The stack
name comes from `SUTURA_PULUMI_STACK` (the machine env sets it to the developer's own) and is
passed to both the config step and pulumi, so a fresh shell needs no "active stack":

```sh
SUTURA_PULUMI_STACK=dev just infra-preview
SUTURA_PULUMI_STACK=dev just infra-up --yes
```

(`just infra-preview` / `just infra-up` configure the stack from the environment and run Pulumi
through the `infra` pixi env. They run as the **developer's gcloud ADC** (the elevated account
from `gcloud auth application-default login`), NOT the BigQuery SA key that the `env.sh` acceptance
leg uses - a self-bootstrapping `up` needs ability to enable APIs and create resources, which the
limited SA key does not have. Override the default ADC path with `GOOGLE_ADC`. The CLI is the
nix-pinned `pulumi` and the SDK the pixi-locked one; a gate in `config-from-env.sh` fails if the
two ever disagree.)

To tear the stack down (so a re-`up` rotates every key), run `just infra-down` - `pulumi
destroy`, as the same ADC, against the same backend. It deletes every resource the stack
created but leaves the GCP APIs **enabled** (deliberately - see the caveats). Preview it first
with `pulumi destroy --preview-only`. Re-create afterwards with `infra-up` then `infra-set`.

## What consumes the outputs

- the `bq-test` environment's secrets/vars (see below), consumed by the `bigquery-acceptance`
  job;
- `principal_a_email` / `principal_b_email` and the row-grant mapping are what the
  two-principal acceptance cell asserts against;
- `workload_audience` (and the pool provider) is the (a) end of a served
  `impersonation-at-source` source, once the exchanging broker is attached to one.

## The `bq-test` GitHub environment

The stack's outputs reach the `bigquery-acceptance` CI job through the `bq-test` GitHub
**environment**. A one-time operator step, `just infra-set`, reads the stack's secret outputs
and pushes them to that environment, so nothing is committed and the CI credential + resource
names follow the stack after a re-`up` (which only rotates the keys). `just infra-set` needs
`gh` authenticated to the repository, and defaults to the `bq-test` environment (override with
`BQ_TEST_ENV`).

**Each row IS the list**, and `cargo xtask check-venues` is what holds it: every name the sync
script pushes appears in its row, every name a row carries is one the script pushes, and every
`vars.SUTURA_BQ_*`, `secrets.SVC_*` or `secrets.SUTURA_BQ_*` a workflow reads is a name the script
can provision. Before that gate existed these were three independent lists with nothing deriving one
from another, and the row below listed five `vars` while the script pushed ten. What the gate cannot
see is whether the environment is actually provisioned - that authority is the GitHub API, and it is
unreachable from the sandbox the gate runs in, so `just infra-up` and `just infra-set` having been
run is still a thing somebody has to know.

| Kind | Names |
| --- | --- |
| `secrets` | `SVC_SUTURUA_BQ_CI` (CI service-account key), `SVC_SUTURUA_BQ_PRINCIPAL_A`, `SVC_SUTURUA_BQ_PRINCIPAL_B`, `SUTURA_BQ_WORKLOAD_AUDIENCE`, `SUTURA_BQ_PRINCIPAL_A_EMAIL`, `SUTURA_BQ_PRINCIPAL_B_EMAIL` |
| `vars` | `SUTURA_BQ_DATASET`, `SUTURA_BQ_TABLE`, `SUTURA_BQ_RLS_DATASET`, `SUTURA_BQ_RLS_TABLE`, `SUTURA_BQ_GROUP_COLUMN`, `SUTURA_BQ_PRINCIPAL_A_ROWS`, `SUTURA_BQ_PRINCIPAL_B_ROWS` |

The three `SUTURA_BQ_` **secrets** are the two-principal cell's identity values, and they are
secrets rather than vars for a reason that is not credential material: each names the acceptance
project or an account in it, a var's value is unmasked wherever a job echoes it, and `just
infra-set` prints every var it sets. A workflow reading one of them must read `secrets.`, not
`vars.`: the `vars` row no longer carries the name, so a provisioning run does not create it and a
`vars.` read is a `check-venues` failure. Whether the live environment still carries a `vars` entry
one of these three left behind is not something this page or that gate can see - the GitHub API is
the only authority for that.

The last five `vars` are the two-principal cell's own: the row-access-policied dataset and table,
the column the two policies filter on, and the grouping value each policy grants. The policied
dataset is deliberately **not** the one the acceptance legs run against - those legs render `CREATE
OR REPLACE TABLE`, which drops a table's row access policies - and the program refuses a
configuration where the two coincide. An `infra-set` that predates them leaves the environment
incomplete, and the consequence is a red `bigquery-acceptance` on every push rather than a skip: the
job fails closed on an unset value deliberately.

The stack creates a dedicated **CI service account** (`ci_sa`), granted project-level
`bigquery.jobUser` and dataset-level `bigquery.dataEditor` on both the stack dataset and the
`ci_dataset` (the already-populated acceptance dataset), so the acceptance/corpus legs can run
under it. A fork's pull request cannot see an environment's secrets, so `bigquery-acceptance`
skips there and runs in-repo, the same `docs/adr/0017` rule.

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
  applying credential (the operator's ADC) holds `serviceusage.services.enable`, since the
  program's own API-bootstrap `Service` resources turn the APIs on.
- **An `infra-down` leaves the APIs enabled** (`disable_on_destroy=False` on the `Service`
  resources): GCP refuses to disable some APIs that still hold resources, and re-enabling is
  slower than leaving it. Re-running `up` reuses them.
