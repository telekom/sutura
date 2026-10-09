---
title: DataFusion (files)
description: Answer questions over a directory of Parquet, CSV or NDJSON files with the in-process engine.
---

# DataFusion (files)

The `files` data system answers questions over a directory of files with DataFusion, inside the
sutura process. The crate is `sutura-exec-datafusion`, and the source kind is `files`. It reads
Parquet, CSV and NDJSON files. It runs the plan directly and writes no SQL. Every build of sutura
has it.

## When to use it

- The data is files in one directory: on a laptop, in a demo, or for one operator.
- No data system login is necessary.
- sutura also uses DataFusion to join the two legs of a federated answer.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and this key:

| Key        | Type          | Default  | Meaning                                    |
| ---------- | ------------- | -------- | ------------------------------------------ |
| `data_dir` | absolute path | required | The directory with one file for each model |

For a model with the table `T`, sutura looks for `T.parquet`, then `T.csv`, then `T.ndjson`. For
each name it also tries the endings `.gz`, `.bz2`, `.xz` and `.zst`. The first file that it finds
is the table. A table name with a qualifier, such as `dataset.table`, is refused.

These runtime keys apply to the engine:

| Key                             | Type    | Default              | Meaning                                                                     |
| ------------------------------- | ------- | -------------------- | --------------------------------------------------------------------------- |
| `runtime.working_set_max_bytes` | integer | `1073741824` (1 GiB) | The memory that the engine may reserve for joins, aggregates and sorts      |
| `runtime.engine_worker_threads` | integer | the CPU count        | The threads of the engine, 1 to 256. Set it in a container with a CPU quota |

The engine never writes rows to disk. A query that needs more memory than
`runtime.working_set_max_bytes` is refused.

## Example

This is the source of the [single player](../../examples/single-player.md) example, as environment
variables:

```yaml
SUTURA__SECURITY__IDENTITY: single-user
SUTURA__SECURITY__SINGLE_USER_BECAUSE: "one operator reading their own files"
SUTURA__SOURCES__LOCAL__KIND: files
SUTURA__SOURCES__LOCAL__DATA_DIR: /examples/data
SUTURA__SOURCES__LOCAL__POSTURE: shared-service-user
```

The same entry as a settings file:

```yaml
sources:
  local:
    kind: "files"
    data_dir: "/srv/sutura/data"
    posture: "shared-service-user"
```

## Identity

This data system supports `shared-service-user` only: DataFusion runs inside the sutura process, so no per-caller identity reaches the source.

The engine reads the files as the operating-system user of the sutura process. The source must
use `posture: shared-service-user`.
