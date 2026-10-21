"""One-shot loader for the metrics-layer example. Python standard library only.

Writes the `orders` and `customers` datasets, their relationship and the certified metric
`revenue` into DataHub, and a DataHub token for sutura to /out/datahub-token. With KEYCLOAK_URL set
it also writes what sutura reads from the realm: /out/jwks.json, /out/exchange-secret and a token of
the `sutura-deployment` client at /out/cube-token.
"""

import base64
import hashlib
import hmac
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

GMS = os.environ["DATAHUB_GMS_URL"]
OUT = "/out"
PLATFORM = "urn:li:dataPlatform:bigquery"
PROPERTY = "sutura"
PROPERTY_URN = f"urn:li:structuredProperty:{PROPERTY}"
RELATIONSHIP = "orders_to_customer"
STRING, NUMBER, DATE = (
    ("STRING", "StringType"),
    ("NUMERIC", "NumberType"),
    ("DATE", "DateType"),
)
TABLES = {
    "orders": (
        "Net revenue orders, in minor units.",
        {
            "order_id": STRING,
            "customer_id": STRING,
            "amount_cents": NUMBER,
            "order_date": DATE,
            "status": STRING,
        },
    ),
    "customers": (
        "The customer dimension.",
        {"customer_id": STRING, "segment": STRING},
    ),
}
# The certified number: the revenue of June 2026 that the `sutura-deployment` client reads.
REVENUE = {
    "model": "orders",
    "description": "Net revenue in minor units, from active orders.",
    "measure": {"simple": {"aggregate": "sum", "column": "amount_cents"}},
    "time_column": "order_date",
    "grains": ["month"],
    "required_filters": [{"equals": {"column": "status", "value": "active"}}],
    "dimensions": [
        {
            "name": "segment",
            "column": "segment",
            "via": RELATIONSHIP,
            "allowed_values": ["retail", "wholesale"],
            "description": "The customer's segment.",
        }
    ],
    "anchor": {
        "range": {"start": "2026-06-01", "end": "2026-07-01"},
        "value": "412345",
    },
}


def request(url, body=None, token=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    data = None if body is None else json.dumps(body).encode()
    with urllib.request.urlopen(
        urllib.request.Request(url, data, headers), timeout=30
    ) as answer:
        return json.load(answer)


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


def datasets():
    return [
        {
            "urn": dataset_urn(table),
            "schemaMetadata": {
                "value": {
                    "schemaName": table,
                    "platform": PLATFORM,
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
        for table, (description, columns) in TABLES.items()
    ]


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
                            "from": dataset_urn("orders"),
                            "fromColumns": ["customer_id"],
                            "to": dataset_urn("customers"),
                            "toColumns": ["customer_id"],
                            "cardinality": "N_ONE",
                        }
                    ],
                }
            },
        }
    ]


def metrics():
    return [
        {
            "urn": f"urn:li:metric:({PLATFORM},orders,revenue)",
            "metricKey": {
                "value": {"platform": PLATFORM, "path": "orders", "id": "revenue"}
            },
            "metricInfo": {
                "value": {
                    "name": "revenue",
                    "expression": {
                        "dialects": [
                            {"dialect": "ANSI_SQL", "expression": "SUM(amount_cents)"}
                        ]
                    },
                }
            },
            "structuredProperties": {
                "value": {
                    "properties": [
                        {
                            "propertyUrn": PROPERTY_URN,
                            "values": [{"string": json.dumps(REVENUE)}],
                        }
                    ]
                }
            },
        }
    ]


def realm(path, form=None):
    url = f"{os.environ['KEYCLOAK_URL']}/realms/metrics-layer/protocol/openid-connect/{path}"
    data = None if form is None else urllib.parse.urlencode(form).encode()
    with urllib.request.urlopen(url, data, timeout=30) as answer:
        return json.load(answer)


def write_out(name, text):
    with open(f"{OUT}/{name}", "w") as file:
        file.write(text)


def keycloak():
    keys = [key for key in realm("certs")["keys"] if key.get("use") == "sig"]
    if not keys:
        sys.exit("the realm publishes no signing key")
    write_out("jwks.json", json.dumps({"keys": keys}))
    write_out("exchange-secret", os.environ["SUTURA_EXCHANGE_SECRET"])
    deployment = realm(
        "token",
        {
            "grant_type": "client_credentials",
            "client_id": "sutura-deployment",
            "client_secret": os.environ["SUTURA_DEPLOYMENT_SECRET"],
        },
    )
    write_out("cube-token", deployment["access_token"])
    print(f"wrote the realm's {len(keys)} signing key(s) and the deployment's token")


def load():
    with open(f"{OUT}/datahub-token", "w") as file:
        file.write(personal_access_token("sutura", hours=24))
    writer = personal_access_token("datahub", hours=1)
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
        ("dataset", datasets(), ["schemaMetadata", "datasetProperties"]),
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
    load()
    if "KEYCLOAK_URL" in os.environ:
        keycloak()
