---
title: Who may see a metric
description: Why a metric and its metadata are one visibility unit, how verified group membership maps to catalog-declared audiences, why an invisible metric is indistinguishable from an unknown one, and why provenance keeps the pinned bundle's digest.
---

# Who may see a metric

Status: **accepted, and nothing is built.** This is the decision before `#148` changes a catalog
shape, request path or transport. Today every verified caller who may invoke a catalog or query
operation sees the whole pinned bundle. There is no visibility declaration, caller-filtered view or
request-time filter in the tree.

## The decision

**A metric is the unit of visibility.** Its name, description, measure, dimensions, declared values,
anchor and structured knowledge are one unit. A dimension, value or glossary phrase cannot be
granted independently of the metric it qualifies. Splitting those pieces would advertise questions
that cannot be asked, or let two callers read different meanings under one metric name.

Knowledge follows its structured referents rather than its prose:

* a glossary entry follows the metric its `Referent` names;
* a caveat is visible only when every metric it refers to is visible;
* a worked example follows the metric in its `Query`;
* an unscoped absence has no metric from which to inherit, so it needs an explicit catalog-wide
  audience and is withheld when none is granted.

The all-referents rule treats a note as authored, atomic content. Removing one hidden referent while
keeping the body could change what the note says. Its cost is that a caveat shared by differently
visible metrics may disappear for a caller who can see only one; the author must split such a caveat
when each audience needs it.

**The catalog declares one audience on each metric, under the definition digest:** either `open`, or
`restricted` with a non-empty set of audience identifiers. Open to every verified caller is explicit;
a missing declaration never means everyone. Changing a metric's audience therefore changes the
pinned bundle just as changing its description does. The same rule applies to the catalog-wide
audience of unscoped knowledge.

**The deployment maps authenticated group-claim values to those audience identifiers.** The token's
scopes continue to decide which operations the caller may invoke. They do not become per-metric
grants: adding a metric must not require adding a scope to the authorization server's deployed
contract. A missing claim, an unmapped value or a mapping that grants no audience produces the empty
restricted view, apart from metadata explicitly declared open to every verified caller. None of
these states means the whole catalog.

This is deliberately two declarations with two owners. The catalog author classifies metadata using
portable audience identifiers; the deployer binds verified identity-provider groups to those
identifiers. The group mapping is deployment policy and is not part of the bundle digest. The caller
cannot supply either half as query input.

For a verified caller `c`, let `granted(c)` be the union of the audiences to which the deployment maps
each value in `c`'s verified group claim. A missing claim and a claim containing only unmapped values
both produce the empty set; an unmapped value in a mixed claim contributes nothing and does not cancel
a mapped value. For a metadata unit `u`, visibility is exactly:

```text
visible(c, u) = audience(u) is open
             or (audience(u) is restricted(A) and A intersects granted(c))
```

For a metric restricted to audience `finance`, the complete decision is:

| Verified group claim | Deployment mapping result | Open metric | Restricted metric |
| --- | --- | --- | --- |
| absent | empty | visible | hidden |
| only unmapped values | empty | visible | hidden |
| mapped to `finance`, with or without unmapped values | includes `finance` | visible | visible |
| mapped, but not to `finance`, with or without unmapped values | excludes `finance` | visible | hidden |

Any mapped group that grants a declared audience is sufficient; unmapped groups neither grant nor
veto visibility. The inheritance rules above are then applied to knowledge after metric visibility is
known. An unscoped absence applies this same predicate to its catalog-wide audience.

## Invisible means absent at both doors

**An invisible metric is unlisted and unaskable.** On authenticated HTTP, catalog rendering and
semantic resolution will consume the same caller-filtered view. Resolving an invisible metric will
therefore take the existing `MetricUnknown` path, exactly as a genuinely absent metric does.

The two refusals are intentionally byte-identical, including status, code and detail. An
authorization-specific refusal would confirm that the requested metric exists. This decision gives
up an actionable distinction at this boundary in order not to provide a metadata-enumeration oracle.
The refusal may echo the name the caller supplied; it must disclose nothing learned from the hidden
definition.

The predicate takes a verified caller, not an optional one. Absence is decided before a view is built,
and differs by surface:

| Surface | When there is no verified caller |
| --- | --- |
| HTTP with `security.inbound` configured | The inbound gate returns `401` before either the catalog or query handler runs. Neither open nor restricted metadata is returned. |
| HTTP without `security.inbound` | This is the explicit single-player posture. Catalog rendering and query resolution use the whole bundle; absence is not interpreted as an empty group claim. |
| `sutura catalog`, `sutura describe` and `sutura prompt` | These are operator-side commands with no request caller and retain the whole bundle. The prompt renderer has no served endpoint today. |
| stdio MCP | The transport cannot establish a caller or receive a token, so `describe_catalog` and `ask_metric` retain the whole bundle. Caller-specific MCP visibility waits for an authenticated transport. |

The implementation must keep the verified-caller and explicit whole-bundle cases distinct. An
`Option<VerifiedCaller>` whose absent arm silently chooses either `open` or unrestricted visibility
would merge deployment posture with failed authentication.

## The digest is the bundle's, not the view's

**The existing definition digest continues to identify the whole immutable pinned bundle from which
the view was projected.** It is not recomputed per caller. `PinnedDefinitions::pin` computes that
digest from the definitions, knowledge and contribution manifest it stores; it does not run anchors.
Later, `verify_and_validate` checks declared keys and runs declared anchors against the already-pinned
bundle before producing the servable wrapper. A projection is neither the content `pin` hashed nor a
bundle that `verify_and_validate` validated, so a per-view digest would define new provenance rather
than preserve the existing one. Anchor checks reproduce declared numbers; they do not attest a
digest.

This makes `definition_digest` an origin-bundle digest, not a digest of the bytes returned by a
filtered catalog response. Because audience declarations live in the catalog, changing a metric's
classification moves that digest. Changing the deployment's group-to-audience mapping does not, and
the digest does not attest that the mapping is correct.

The accepted disclosure is correlation: a caller can tell that the origin bundle changed even when
only hidden metadata changed, and can compare equality with a candidate bundle whose contents it
already knows. The digest does not reveal the hidden names by itself. Avoiding that signal would
require a per-view digest or a second opaque version, neither of which would be the provenance an
answer carries today.

## No request-time catalog

**`SemanticCatalog::load()` stays free of `RequestContext`.** A process loads and pins one immutable
bundle. Per request, verified group membership is mapped to audiences and a view borrows from that
bundle; it neither loads definitions nor caches a caller-specific copy. Filtering is metadata access
over pinned content, never a new source of definitions.

The later type must make that boundary structural: no constructor taking a path, catalog port or I/O
handle, and no renderer or resolver that can bypass the filtered input when inbound visibility is
configured. This record does not claim such a type exists yet.

## Alternatives declined

**A scope per metric.** Declined because scopes already govern operations and are pinned as part of
that deployed contract. It would turn every catalog addition into authorization-server work and
invite scope to be misdescribed as row access.

**Independent dimension, value or phrase grants.** Declined because those objects qualify one metric.
Partial visibility would multiply declaration and refusal states while permitting a caller to see a
metric whose usable or explanatory metadata had been removed.

**A settings-only list of metric names.** Declined because a metadata-classification change would
leave the bundle digest unmoved. Portable audience identifiers belong with the authored metadata;
only their deployment-specific group mapping belongs in settings.

**A digest of each caller's projection.** Declined because `pin` did not compute it, and the projection
is not a bundle `verify_and_validate` validated. It would make two callers report different
provenance for the same authored metric; anchor checks do not attest either digest.

**A distinct authorization refusal.** Declined because it confirms the hidden metric. Invisible and
unknown deliberately share `MetricUnknown` instead.

**Loading a catalog per caller.** Declined because it reopens the request path to metadata I/O and
allows the digest carried by an answer to describe definitions other than those that produced it.

## Consequences and limits

* A visibility declaration is a hard catalog-shape change. The implementation must parse it through
  a non-empty type and fail closed rather than default a missing field to unrestricted access.
* The catalog declaration moves the definition digest; the deployment mapping does not. Both facts
  must be stated wherever the filtered response's digest is described.
* One borrowed view can make advertisement and invocation agree. Two separate predicates would be a
  state in which a metric can be hidden at one door and used through the other.
* Authored free text can still mention a model, column or hidden metric. Existing types constrain
  structured referents, not prose, so metadata visibility is not a content-redaction mechanism.
* This is **metadata access only**. It changes no source's configured `SourcePosture`, credential path
  or rows. A question continues through whatever posture and credential path that source already
  uses, shared or impersonating; visibility neither selects the deployment identity nor proves that a
  source executed as the caller. It is neither row-level authorization nor evidence that the second
  leg of impersonation ran.
* No visibility declaration, caller-filtered type, refusal-path change or transport integration is
  implemented by this record.
