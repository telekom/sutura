---
title: Cube
description: Read the cubes and their dimensions from a Cube metrics serving layer.
---

# Cube

[Cube](https://cube.dev) is a metrics serving layer. It serves each certified metric to BI tools,
REST clients and other established tools. The Cube adapter reads the data model from the Cube
metadata API. The crate is `sutura-catalog-cube`.

## When to use it

- Your organization runs Cube, and BI tools or REST clients use its metrics.
- You want agents to see the same certified definitions that Cube serves.

## How the model maps

| In Cube                              | In sutura                                                                                 |
| ------------------------------------ | ----------------------------------------------------------------------------------------- |
| A cube or a view                     | A model on the Cube source. The model and its table have the name of the cube.            |
| A dimension                          | A column of that model, with the short name of the dimension and the Cube type of it.     |
| A dimension with `primary_key: true` | A column of the primary key of that model.                                                |
| The description of a cube            | The description of the model. Each cube must have a description.                          |
| A measure, a segment, a join         | These stay in Cube. Cube computes them, and sutura does not compute a Cube metric itself. |

Cube shows a primary key dimension in its metadata API only when the dimension has `public: true`.
Set it on each primary key dimension.

## What Cube needs

- sutura sends `GET /cubejs-api/v1/meta` to the root URL of the Cube server.
- Each request has the header `Authorization: Bearer <token>`. The token is a JSON Web Token that
  Cube accepts: signed with `CUBEJS_API_SECRET`, or verified with the key set at `CUBEJS_JWK_URL`.
- Use `https` for a remote Cube. sutura uses `http` only for a loopback IP address.

## Example model

`just dev-up-cube` starts Cube with the model in `examples/cube/model`. The model puts the
[single player](../../examples/single-player.md) data into five cubes.
