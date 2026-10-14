"""One-shot loader for the federated example. Python standard library only.

Writes the `fct_subscription_monthly` dataset on Postgres, the `dim_customer` dataset on
ClickHouse, their relationship and the certified metric `recurring_revenue` into DataHub, and a
DataHub token for sutura to /out/datahub-token. The DataHub helpers are those of the metrics-layer
example's loader, mounted at /metrics-layer.
"""

import importlib.util
import json

spec = importlib.util.spec_from_file_location("metrics_layer", "/metrics-layer/load.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)

FACT, DIMENSION = "fct_subscription_monthly", "dim_customer"
RELATIONSHIP = "subscription_customer"
STRING, NUMBER, DATE, BOOLEAN = (
    ("VARCHAR", "StringType"),
    ("NUMERIC", "NumberType"),
    ("DATE", "DateType"),
    ("BOOLEAN", "BooleanType"),
)
# Each dataset: its platform, its description and its columns.
TABLES = {
    FACT: (
        "postgres",
        "Monthly recurring revenue per subscription.",
        {
            "month": DATE,
            "subscription_key": NUMBER,
            "customer_key": NUMBER,
            "product_key": NUMBER,
            "status": STRING,
            "mrr_cents": NUMBER,
            "churned_in_month": BOOLEAN,
            "contract_term": STRING,
        },
    ),
    DIMENSION: (
        "clickhouse",
        "The customer dimension.",
        {
            "customer_key": NUMBER,
            "customer_id": STRING,
            "segment": STRING,
            "region": STRING,
        },
    ),
}
RECURRING_REVENUE = {
    "model": FACT,
    "measure": {"simple": {"aggregate": "sum", "column": "mrr_cents"}},
    "time_column": "month",
    "grains": ["month"],
    "required_filters": [{"equals": {"column": "status", "value": "active"}}],
    "dimensions": [
        {
            "name": "segment",
            "column": "segment",
            "via": RELATIONSHIP,
            "allowed_values": ["business", "consumer", "wholesale"],
        }
    ],
    "anchor": {
        "range": {"start": "2026-06-01", "end": "2026-07-01"},
        "value": "202121",
    },
}


def platform_urn(platform):
    return f"urn:li:dataPlatform:{platform}"


def dataset_urn(table):
    return f"urn:li:dataset:({platform_urn(TABLES[table][0])},{table},PROD)"


def datasets():
    return [
        {
            "urn": dataset_urn(table),
            "schemaMetadata": {
                "value": {
                    "schemaName": table,
                    "platform": platform_urn(platform),
                    "version": 0,
                    "hash": "",
                    "platformSchema": {
                        "com.linkedin.schema.OtherSchema": {"rawSchema": ""}
                    },
                    "fields": [
                        {
                            "fieldPath": column,
                            "nativeDataType": native,
                            "type": {"type": {f"com.linkedin.schema.{kind}": {}}},
                        }
                        for column, (native, kind) in columns.items()
                    ],
                }
            },
            "datasetProperties": {"value": {"description": description}},
        }
        for table, (platform, description, columns) in TABLES.items()
    ]


def relationships():
    return [
        {
            "urn": f"urn:li:semanticModel:({platform_urn('postgres')},PROD,{RELATIONSHIP})",
            "semanticModelInfo": {
                "value": {
                    "name": RELATIONSHIP,
                    "relationships": [
                        {
                            "name": RELATIONSHIP,
                            "from": dataset_urn(FACT),
                            "fromColumns": ["customer_key"],
                            "to": dataset_urn(DIMENSION),
                            "toColumns": ["customer_key"],
                            "cardinality": "N_ONE",
                        }
                    ],
                }
            },
        }
    ]


def metrics():
    platform = platform_urn("postgres")
    return [
        {
            "urn": f"urn:li:metric:({platform},{FACT},recurring_revenue)",
            "metricKey": {
                "value": {"platform": platform, "path": FACT, "id": "recurring_revenue"}
            },
            "metricInfo": {
                "value": {
                    "name": "recurring_revenue",
                    "expression": {
                        "dialects": [
                            {"dialect": "ANSI_SQL", "expression": "SUM(mrr_cents)"}
                        ]
                    },
                }
            },
            "structuredProperties": {
                "value": {
                    "properties": [
                        {
                            "propertyUrn": base.PROPERTY_URN,
                            "values": [{"string": json.dumps(RECURRING_REVENUE)}],
                        }
                    ]
                }
            },
        }
    ]


def load():
    with open(f"{base.OUT}/datahub-token", "w") as file:
        file.write(base.personal_access_token("sutura", hours=24))
    writer = base.personal_access_token("datahub", hours=1)
    writes = [
        # GMS answers 401 to a token whose user does not exist, so sutura's user comes first.
        (
            "corpuser",
            [
                {
                    "urn": "urn:li:corpuser:sutura",
                    "corpUserInfo": {"value": {"active": True}},
                }
            ],
            None,
        ),
        ("structuredproperty", base.property_definition(), None),
        ("dataset", datasets(), ["schemaMetadata", "datasetProperties"]),
        ("semanticModel", relationships(), ["semanticModelInfo"]),
        ("metric", metrics(), ["metricInfo", "structuredProperties"]),
    ]
    for entity, bodies, aspects in writes:
        base.write(f"{base.GMS}/openapi/v3/entity/{entity}?async=false", bodies, writer)
        if aspects:
            base.wait_until_indexed(
                writer, entity, aspects, {body["urn"] for body in bodies}
            )
        print(f"loaded {len(bodies)} {entity} entities")


if __name__ == "__main__":
    load()
