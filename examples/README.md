# Examples

The walkthroughs are on the [documentation site](https://telekom.github.io/sutura/latest/examples/), and
in [`docs/examples/`](../docs/examples/index.md). A test runs every directory here.

| Directory             | What it is                                                                                              |
| --------------------- | ------------------------------------------------------------------------------------------------------- |
| `single-player/`      | A markdown catalog over local CSV files, for one user. [Walkthrough](../docs/examples/single-player.md) |
| `raw-sql/`            | The `run_sql` tool, which is off by default, over a Postgres source                                     |
| `authored-sql/`       | A catalog with one metric written as SQL under `authored_sql:`                                          |
| `multi-player/`       | A served deployment with a DataHub catalog and per-caller identity                                      |
| `cube/`               | The markdown catalog of `single-player/` over a Cube source, with the Cube model and its data           |
| `metrics-layer/`      | A DataHub catalog over Cube as the only source, asked with each caller's own Keycloak token             |
| `federated/`          | A DataHub catalog over Postgres and ClickHouse, asked one question that reads both                      |
| `tardis/`             | Settings behind the gateway of the Open Telekom Integration Platform                                    |
| `okf/`                | Catalog files for the OKF reader                                                                        |
| `datacontract/`       | Catalog files for the Open Data Contract Standard reader                                                |
| `demo-chatinterface/` | The container behind `just demo`. [The local chat demo](../docs/demo.md)                                |

All data is synthetic.
