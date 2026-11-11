---
title: Native dictionary linked metadata
description: Native metadata selection and cross-catalog ownership without widening the planned column set.
---

# Native dictionary linked metadata

Status: **accepted**.

Native dictionary mode reads visible metadata under the catalog login, with fixed queries,
bound schema values and bounded case-sensitive identifier patterns. Documentation views
remain the default. Unsupported types refuse rather than lose precision.

A catalog's kind and capabilities are type-level declarations. An explicit native RDBMS
plus Markdown composition arm therefore links concrete unlinked declarations, rather than
wrapping arbitrary adapters under one misleading catalog type. Contributor ownership and
capability fidelity are checked before references resolve and the joined bundle is pinned.

A successful native read supplies the effective physical table/column allowlist for its
configured source alias. Every contributor's models are checked against that authority,
including semantic aliases; duplicate native authorities refuse. Configured source aliases
remain distinct authorities even when operators point them at the same database. Raw
pass-through SQL remains governed by database grants and does not inherit exclusions.

Boot and refresh use the same loader. A failed read, selection, composition or validation
leaves the whole previous bundle published. Metadata byte checks follow allocation and
provide no process-memory or hard elapsed-time bound. Live proof covers CI fixtures only;
source review and a green compile are not behavioral verification.
