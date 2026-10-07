"""One-shot loader for the multi-player example. Python standard library only.

`load.py jwks` writes the realm's public signing keys for sutura and for Pulumi.
`load.py` (no argument) also writes sutura's two secrets and loads the dbt manifest into DataHub.
"""

import base64
import hashlib
import hmac
import json
import os
import sys
import time
import urllib.error
import urllib.request

KEYCLOAK = os.environ["KEYCLOAK_URL"]
GMS = os.environ["DATAHUB_GMS_URL"]
RUN = "/run/sutura"
OUT = "/out"
SUTURA_UID = 65532
PLATFORM = "urn:li:dataPlatform:bigquery"
PROPERTY = "sutura"
PROPERTY_URN = f"urn:li:structuredProperty:{PROPERTY}"


def request(url, body=None, token=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    data = None if body is None else json.dumps(body).encode()
    with urllib.request.urlopen(
        urllib.request.Request(url, data, headers), timeout=30
    ) as answer:
        return json.load(answer)


def wait_for(url, seconds=300):
    deadline = time.monotonic() + seconds
    while True:
        try:
            return request(url)
        except (urllib.error.URLError, ConnectionError) as cause:
            if time.monotonic() > deadline:
                sys.exit(f"{url} did not answer within {seconds} s: {cause}")
            time.sleep(3)


def write_for_sutura(name, text):
    path = f"{RUN}/{name}"
    with open(path, "w") as file:
        file.write(text)
    os.chown(path, SUTURA_UID, SUTURA_UID)
    os.chmod(path, 0o400)


def jwks():
    certs = wait_for(f"{KEYCLOAK}/realms/sutura-example/protocol/openid-connect/certs")
    signing = {"keys": [key for key in certs["keys"] if key.get("use") == "sig"]}
    if not signing["keys"]:
        sys.exit("the realm publishes no signing key")
    text = json.dumps(signing)
    write_for_sutura("jwks.json", text)
    if os.path.isdir(OUT):
        with open(f"{OUT}/jwks.json", "w") as file:
            file.write(text)
    print(f"wrote the realm's {len(signing['keys'])} public signing key(s)")


def b64(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def personal_access_token(actor, hours):
    """A stateless DataHub access token, signed with the GMS token-service key (HS256)."""
    header = b64(json.dumps({"alg": "HS256", "typ": "JWT"}).encode())
    claims = {
        "version": "1",
        "type": "PERSONAL",
        "actorType": "USER",
        "actorId": actor,
        "sub": f"urn:li:corpuser:{actor}",
        "exp": int(time.time()) + hours * 3600,
    }
    body = b64(json.dumps(claims).encode())
    key = os.environ["DATAHUB_TOKEN_SIGNING_KEY"].encode()
    signature = hmac.new(key, f"{header}.{body}".encode(), hashlib.sha256).digest()
    return f"{header}.{body}.{b64(signature)}"


def dataset_urn(table):
    return f"urn:li:dataset:({PLATFORM},{table},PROD)"


def datasets(manifest):
    """Every documented seed or model of the dbt manifest, as one DataHub dataset."""
    out = []
    for node in manifest["nodes"].values():
        if node["resource_type"] not in ("seed", "model") or not node["columns"]:
            continue
        fields = [
            {
                "fieldPath": column["name"],
                "nativeDataType": column["data_type"],
                "description": column["description"],
                "type": {"type": {"com.linkedin.schema.StringType": {}}},
            }
            for column in node["columns"].values()
        ]
        out.append(
            {
                "urn": dataset_urn(node["name"]),
                "schemaMetadata": {
                    "value": {
                        "schemaName": node["name"],
                        "platform": PLATFORM,
                        "version": 0,
                        "hash": node["checksum"]["checksum"],
                        "platformSchema": {
                            "com.linkedin.schema.OtherSchema": {"rawSchema": ""}
                        },
                        "fields": fields,
                    }
                },
                "datasetProperties": {"value": {"description": node["description"]}},
            }
        )
    return out


RELATIONSHIP = "subscription_customer"
SEGMENT = {
    "name": "segment",
    "column": "segment",
    "via": RELATIONSHIP,
    "allowed_values": ["business", "consumer", "wholesale"],
    "description": "The commercial segment of the customer.",
}
TERM = {
    "name": "contract_term",
    "column": "contract_term",
    "allowed_values": ["annual", "monthly"],
    "description": "The contract length. The row access policies split on it.",
}
METRICS = {
    "recurring_revenue": (
        "SUM(mrr_cents)",
        {"simple": {"aggregate": "sum", "column": "mrr_cents"}},
        "Recurring revenue in the month, in minor units, from active subscriptions only.",
    ),
    "active_subscriptions": (
        "COUNT(DISTINCT subscription_key)",
        {"simple": {"aggregate": "count_distinct", "column": "subscription_key"}},
        "How many subscriptions were active at the end of the month.",
    ),
}


def relationships():
    return [
        {
            "urn": f"urn:li:semanticModel:({PLATFORM},PROD,{RELATIONSHIP})",
            "semanticModelInfo": {
                "value": {
                    "name": RELATIONSHIP,
                    "relationships": [
                        {
                            "name": RELATIONSHIP,
                            "from": dataset_urn("fct_subscription_monthly"),
                            "fromColumns": ["customer_key"],
                            "to": dataset_urn("dim_customer"),
                            "toColumns": ["customer_key"],
                            "cardinality": "N_ONE",
                        }
                    ],
                }
            },
        }
    ]


def metrics():
    out = []
    for name, (expression, measure, description) in METRICS.items():
        document = {
            "model": "fct_subscription_monthly",
            "description": description,
            "measure": measure,
            "time_column": "month",
            "grains": ["month"],
            "required_filters": [{"equals": {"column": "status", "value": "active"}}],
            "dimensions": [SEGMENT, TERM],
        }
        out.append(
            {
                "urn": f"urn:li:metric:({PLATFORM},fct_subscription_monthly,{name})",
                "metricKey": {
                    "value": {
                        "platform": PLATFORM,
                        "path": "fct_subscription_monthly",
                        "id": name,
                    }
                },
                "metricInfo": {
                    "value": {
                        "name": name,
                        "expression": {
                            "dialects": [
                                {"dialect": "ANSI_SQL", "expression": expression}
                            ]
                        },
                    }
                },
                "structuredProperties": {
                    "value": {
                        "properties": [
                            {
                                "propertyUrn": PROPERTY_URN,
                                "values": [{"string": json.dumps(document)}],
                            }
                        ]
                    }
                },
            }
        )
    return out


def property_definition():
    return [
        {
            "urn": PROPERTY_URN,
            "propertyDefinition": {
                "value": {
                    "qualifiedName": PROPERTY,
                    "displayName": PROPERTY,
                    "valueType": "urn:li:dataType:datahub.string",
                    "cardinality": "SINGLE",
                    "entityTypes": ["urn:li:entityType:datahub.metric"],
                    "description": "The certified metric document sutura reads.",
                }
            },
        }
    ]


def wait_until_indexed(token, entity, aspects, wanted, seconds=60):
    query = "&".join(f"aspects={aspect}" for aspect in aspects)
    deadline = time.monotonic() + seconds
    while True:
        page = request(
            f"{GMS}/openapi/v3/entity/{entity}?{query}&count=1000", token=token
        )
        seen = {
            item["urn"]
            for item in page["entities"]
            if all(aspect in item for aspect in aspects)
        }
        if wanted <= seen:
            return
        if time.monotonic() > deadline:
            sys.exit(
                f"DataHub did not index {sorted(wanted - seen)} within {seconds} s"
            )
        time.sleep(1)


def write(url, bodies, token, seconds=180):
    """GMS answers 403 until it has loaded its policies, which ends after it reports healthy."""
    deadline = time.monotonic() + seconds
    while True:
        try:
            return request(url, bodies, token)
        except urllib.error.HTTPError as cause:
            if cause.code != 403 or time.monotonic() > deadline:
                raise
            time.sleep(3)


def load():
    jwks()
    write_for_sutura("exchange-secret", os.environ["SUTURA_EXCHANGE_SECRET"])
    write_for_sutura("datahub-token", personal_access_token("sutura", hours=24))
    writer = personal_access_token("datahub", hours=1)
    with open("/dbt/manifest.json") as file:
        manifest = json.load(file)
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
        ("structuredproperty", property_definition(), None),
        ("dataset", datasets(manifest), ["schemaMetadata", "datasetProperties"]),
        ("semanticModel", relationships(), ["semanticModelInfo"]),
        ("metric", metrics(), ["metricInfo", "structuredProperties"]),
    ]
    for entity, bodies, aspects in writes:
        write(f"{GMS}/openapi/v3/entity/{entity}?async=false", bodies, writer)
        if aspects:
            wait_until_indexed(
                writer, entity, aspects, {body["urn"] for body in bodies}
            )
        print(f"loaded {len(bodies)} {entity} entities")


if __name__ == "__main__":
    jwks() if sys.argv[1:] == ["jwks"] else load()
