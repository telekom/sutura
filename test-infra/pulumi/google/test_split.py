"""Check the declared resource graph offline; this does not prove BigQuery access."""

import asyncio
import json
import runpy
import sys
from pathlib import Path

import pulumi
from pulumi.runtime import MockCallArgs, MockResourceArgs, Mocks


class Resources(Mocks):
    def __init__(self) -> None:
        self.resources: dict[str, MockResourceArgs] = {}

    def new_resource(self, args: MockResourceArgs) -> tuple[str, dict]:
        self.resources[args.name] = args
        return args.name, {
            **args.inputs,
            "uniqueId": args.name,
            "email": f"{args.name}@example.com",
            "creationTime": 1,
        }

    def call(self, args: MockCallArgs) -> dict:
        if args.token == "gcp:bigquery/getDataset:getDataset":
            return {"datasetId": args.args["datasetId"], "location": "EU"}
        return {"projectId": "fixture-project", "number": "123456789"}


resources = Resources()
asyncio.set_event_loop(asyncio.new_event_loop())
pulumi.runtime.set_mocks(resources, project="fixture", stack="fixture")
pulumi.runtime.set_all_config(
    {
        f"fixture:{key}": value
        for key, value in {
            "project": "fixture-project",
            "region": "EU",
            "dataset": "policied",
            "table": "rows",
            "group_column": "group_id",
            "ci_dataset": "acceptance",
            "ci_table": "rows",
            "cross_dataset": "cross_resource",
            "cross_dataset_location": "EU",
            "workload_pool_id": "fixture-pool",
            "workload_provider_id": "fixture-provider",
            "workload_issuer_uri": "https://accounts.example.com",
        }.items()
    }
)


@pulumi.runtime.test
def declare() -> None:
    runpy.run_path(
        sys.argv[1] if len(sys.argv) > 1 else Path(__file__).with_name("__main__.py")
    )


declare()
graph = resources.resources
assert graph["split-dataset"].inputs["datasetId"] == "policied_split"
for fixture, count, subjects in (
    ("shared", 600_000, {"sa-a", "sa-b"}),
    ("a_only", 1, {"sa-a"}),
):
    table = graph[f"split-{fixture}"].inputs
    assert table["datasetId"] == "policied_split"
    query = graph[f"split-{fixture}-seed"].inputs["query"]
    expected_type = "STRING" if fixture == "shared" else "INTEGER"
    assert json.loads(table["schema"]) == [
        {
            "name": "id",
            "type": expected_type,
            "mode": "NULLABLE" if fixture == "shared" else "REQUIRED",
        }
    ]
    expected_query = (
        Path(__file__).with_name("split-shared.sql").read_text(encoding="utf-8")
        if fixture == "shared"
        else f"SELECT id FROM UNNEST(GENERATE_ARRAY(1, {count})) AS id"
    )
    assert query["query"] == expected_query
    assert query["destinationTable"]["tableId"] == table["tableId"]
    assert query["writeDisposition"] == "WRITE_TRUNCATE"
    readers = {
        resource.inputs["member"].rsplit("/", 1)[-1]
        for resource in graph.values()
        if resource.typ == "gcp:bigquery/iamMember:IamMember"
        and resource.inputs["datasetId"] == "policied_split"
        and resource.inputs["tableId"] == table["tableId"]
    }
    assert readers == subjects, (fixture, readers)
for principal in ("a", "b"):
    assert (
        graph[f"principal-{principal}-readsessionuser"].inputs["role"]
        == "roles/bigquery.readSessionUser"
    )
print("split fixtures: resource graph ok; live access not measured")
