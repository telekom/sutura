"""Test-grade Google identity E2E infrastructure for sutura.

This provisions the (a) Google WIF path and the (3) two-principal row-access
cell from issue #81:

  * two distinct service accounts with *different* BigQuery row-level grants,
    so the same question answered under each principal returns different rows -
    the mechanism issue #81 wants proven in CI (the isolation itself is BigQuery
    row-level IAM; sutura's part is only that each job runs under its own bearer);
  * a dataset and a table with a grouping column, plus two BigQuery ROW ACCESS
    POLICIES on that column granting the two principals disjoint rows - a
    first-class `RowAccessPolicy` resource since pulumi_gcp 9.x, no separate gcp
    CLI;
  * the APIs the stack needs are ENABLED as Pulumi `Service` resources first, so
    `up` self-bootstraps a fresh project (the credential still holds
    `serviceusage.services.enable`);
  * a Google Workload Identity Federation pool + OIDC provider (accepting `jwt`
    subject tokens), so a subject's token can be verified and exchanged at Google
    STS (the (a) token path).

NOTHING here is a real identifier: every value comes from Pulumi config, and the
examples are placeholders. The exported private keys are secrets - capture them
with `pulumi stack output --show-secrets` and store them as environment secrets;
never commit them. Per the repository's caller-facing rule, real project/pool
names never belong in these files.
"""

import json

import pulumi
import pulumi_gcp as gcp

cfg = pulumi.Config()

project = cfg.require("project")
region = cfg.require("region")
dataset_id = cfg.require("dataset")
table_id = cfg.require("table")
group_column = cfg.require("group_column")

# Row-grant mapping. The two principals must see DIFFERENT rows for the cell to
# mean anything: that is what BigQuery's row-level IAM enforces, and the mapping
# here is the test-grade stand-in for the real grant. Change the values per
# deployment; they are data, not logic.
principal_a_rows = cfg.get("principal_a_rows") or "a"
principal_b_rows = cfg.get("principal_b_rows") or "b"

# The dataset location (may be a multi-region like `EU`) and the provider's COMPUTE region/zone are
# separate: BigQuery takes its own `location`, while the GCP provider uses a compute region/zone to
# build its resource-identity map. When the provider is given BOTH an explicit `region` AND `zone`
# it does not enumerate compute regions - it is fully qualified - so it never needs
# `compute.regions.list`, which is what produced the `403 ... regions.list ... forbidden` warning
# on a credential that reads BigQuery/IAM only. Set `provider_region`/`provider_zone` to concrete
# compute values (e.g. europe-west3 / europe-west3-a); they fall back to the dataset `region`.
gcp_provider = gcp.Provider(
    "provider",
    project=project,
    region=cfg.get("provider_region") or region,
    zone=cfg.get("provider_zone"),
)

# --------------------------------------------------------------------------- #
# API bootstrap - `up` enables what it needs on a fresh project, no separate gcloud CLI.
# --------------------------------------------------------------------------- #
# Each API the stack touches (identities on `iam`, the dataset/table on `bigquery`) is turned
# on as a Pulumi resource first, and every consumer below waits on the enabling call via
# `depends_on`. serviceusage.googleapis.com powers the ENABLING call itself, so it is enabled
# FIRST and bigquery/iam depend on it - without it, `up` fails on a fresh project with
# `SERVICE_DISABLED` on the enable call, which is exactly what a disabled Service Usage API
# produces. The applying credential still needs `serviceusage.services.enable`; self-bootstrapping
# moves that ONE grant into the credential, which is the same class of trust the provider key is.
_usage = gcp.projects.Service(
    "api-serviceusage.googleapis.com",
    project=project,
    service="serviceusage.googleapis.com",
    disable_on_destroy=False,
    opts=pulumi.ResourceOptions(provider=gcp_provider),
)
API_BOOTSTRAP = [_usage]
for _api in ["bigquery.googleapis.com", "iam.googleapis.com"]:
    API_BOOTSTRAP.append(
        gcp.projects.Service(
            "api-" + _api,
            project=project,
            service=_api,
            disable_on_destroy=False,
            opts=pulumi.ResourceOptions(provider=gcp_provider, depends_on=API_BOOTSTRAP),
        )
    )


def sutura_name(cfg: pulumi.Config, leaf: str) -> str:
    """A deterministic resource id built from config, so nothing hardcoded enters the tree.

    Prefixing by the stack name keeps two stacks (e.g. the (b) enterprise IdP) in
    one project from colliding on resource IDs.
    """
    stack = pulumi.get_stack()
    prefix = f"{stack}-{leaf}"
    # Service account account_ids are 6..30 chars, lowercase alphanumerics + dashes.
    # Trim to stay inside the limit; collisions across two stacks differing only at
    # the tail are avoided by keeping the stack name short.
    return prefix[:30]

# --------------------------------------------------------------------------- #
# (3) Two principals, one table, disjoint rows
# --------------------------------------------------------------------------- #

# Two service accounts. Their GRANTS differ (set by the row access policy below);
# sutura's part is only that each job submits under its own bearer.
sa_a = gcp.serviceaccount.Account(
    "sa-a",
    account_id=sutura_name(cfg, "sa-a"),
    display_name="sutura identity test principal A",
    opts=pulumi.ResourceOptions(provider=gcp_provider, depends_on=API_BOOTSTRAP),
)
sa_b = gcp.serviceaccount.Account(
    "sa-b",
    account_id=sutura_name(cfg, "sa-b"),
    display_name="sutura identity test principal B",
    opts=pulumi.ResourceOptions(provider=gcp_provider, depends_on=API_BOOTSTRAP),
)

# The dataset and the table. The schema carries a DATE column (for the time
# bucket the generated statements project), an INT64 amount, and a STRING
# grouping column for the row access policy.
dataset = gcp.bigquery.Dataset(
    "dataset",
    dataset_id=dataset_id,
    location=region,
    opts=pulumi.ResourceOptions(provider=gcp_provider),
)

schema = json.dumps(
    [
        {"name": "day", "type": "DATE", "mode": "REQUIRED"},
        {"name": "amount", "type": "INT64", "mode": "REQUIRED"},
        {"name": group_column, "type": "STRING", "mode": "REQUIRED"},
    ]
)

table = gcp.bigquery.Table(
    "table",
    dataset_id=dataset.dataset_id,
    table_id=table_id,
    schema=schema,
    deletion_protection=False,
    opts=pulumi.ResourceOptions(provider=gcp_provider, depends_on=[dataset, *API_BOOTSTRAP]),
)

# The isolation: principal A is granted rows where the grouping column equals A's value,
# principal B where it equals B's. Disjoint by construction. Two separate policies (one per
# principal) so each grant is stated on its own line. `grantees` is the IAM member shape
# (`serviceAccount:<email>`), taken from the SA's own `member` output; the SQL filter is the
# row predicate. This is a first-class resource since pulumi_gcp 9.x - no separate gcp CLI.
gcp.bigquery.RowAccessPolicy(
    "rap-a",
    dataset_id=dataset.dataset_id,
    table_id=table.table_id,
    policy_id=sutura_name(cfg, "rap-a"),
    grantees=[sa_a.member],
    filter_predicate=f"{group_column} = '{principal_a_rows}'",
    opts=pulumi.ResourceOptions(provider=gcp_provider, depends_on=[table]),
)
gcp.bigquery.RowAccessPolicy(
    "rap-b",
    dataset_id=dataset.dataset_id,
    table_id=table.table_id,
    policy_id=sutura_name(cfg, "rap-b"),
    grantees=[sa_b.member],
    filter_predicate=f"{group_column} = '{principal_b_rows}'",
     opts=pulumi.ResourceOptions(provider=gcp_provider, depends_on=[table]),
)

# Keys are the long-lived bearer each CI run uses. Exported as secrets; never
# written into the repository.
key_a = gcp.serviceaccount.Key(
    "key-a",
    service_account_id=sa_a.email,
    key_algorithm="KEY_ALG_RSA_2048",
    opts=pulumi.ResourceOptions(provider=gcp_provider),
)
key_b = gcp.serviceaccount.Key(
    "key-b",
    service_account_id=sa_b.email,
    key_algorithm="KEY_ALG_RSA_2048",
    opts=pulumi.ResourceOptions(provider=gcp_provider),
)

# --------------------------------------------------------------------------- #
# (4a) Google Workload Identity Federation - the (a) token path
# --------------------------------------------------------------------------- #
# A workload identity pool + OIDC provider. **Workload, not workforce** - this is
# the shape sutura's broker (`StsOverHttp`) exchanges against: RFC 8693 with a `jwt`
# subject token (`urn:ietf:params:oauth:token-type:jwt`), which workload-pool OIDC
# providers accept and workforce pools do not (workforce takes `id_token`/`saml2` and
# requires a `clientId` plus `webSsoConfig` - issue #87 draws that line). The exported
# audience is what sutura declares as sources.<alias>.workload_identity.audience.
#
# The issuer is config, and so is the allowed audience: the JWTs the caller presents
# must be signed by `issuer_uri` and carry an `aud` in `allowed_audiences`. Google as
# the issuer means the audience is a Google OAuth client id the id_token was minted for.

workload_pool = gcp.iam.WorkloadIdentityPool(
    "workload-pool",
    workload_identity_pool_id=cfg.require("workload_pool_id"),
    display_name="sutura identity test workload pool",
    opts=pulumi.ResourceOptions(provider=gcp_provider, depends_on=API_BOOTSTRAP),
)

workload_provider = gcp.iam.WorkloadIdentityPoolProvider(
    "workload-provider",
    workload_identity_pool_id=workload_pool.workload_identity_pool_id,
    workload_identity_pool_provider_id=cfg.require("workload_provider_id"),
    display_name="OIDC",
    attribute_mapping={
        "google.subject": "assertion.sub",
        "attribute.principal": "assertion.sub",
    },
    oidc=gcp.iam.WorkloadIdentityPoolProviderOidcArgs(
        issuer_uri=cfg.require("workload_issuer_uri"),
        allowed_audiences=cfg.require_object("workload_allowed_audiences"),
        # Disabled: the test mints its own JWTs for the two subjects (different `sub`s)
        # rather than holding a signing key Google could verify for issuing.
    ),
    opts=pulumi.ResourceOptions(provider=gcp_provider, depends_on=[workload_pool]),
)

audience = pulumi.Output.concat(
    "//iam.googleapis.com/",
    "projects/",
    project,
    "/locations/global/workloadIdentityPools/",
    workload_pool.workload_identity_pool_id,
    "/providers/",
    workload_provider.workload_identity_pool_provider_id,
)


# --------------------------------------------------------------------------- #
# Outputs - the values the CI job and sutura-config consume. Keys are secrets.
# --------------------------------------------------------------------------- #

pulumi.export("principal_a_email", sa_a.email)
pulumi.export("principal_b_email", sa_b.email)
pulumi.export("principal_a_key", key_a.private_key)
pulumi.export("principal_b_key", key_b.private_key)
pulumi.export("dataset", dataset.dataset_id)
pulumi.export("table", table.table_id)
pulumi.export("workload_audience", audience)
