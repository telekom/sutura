---
title: A remote leg runs as the subject of a token exchanged for the remote node
description: How one sutura node runs a federated leg at another (#137 step 2). A typed leg crosses as one Arrow Flight call. The caller reaches the remote node as a token the operator's issuer exchanged for that node's audience, and the remote node runs the leg as that token's subject. It checks the leg against its own certified catalog. Never the caller's raw token, never a node-signed assertion, never the deployment.
---

# A remote leg runs as the subject of a token exchanged for the remote node

Status: **accepted**, 2026-10-17. The owner chose option (a) for each of the seven questions below.
It answers #137 step 2. It applies the worker-credential owner decision of 2026-10-12
(`docs/adr/20261010140559-datafusion-federates-the-sql-tool-and-the-metrics.md:420-423`) to a
node. Of that decision's "short-lived", only the issuer holds it: see *Limits*. Nothing here is
built or run. This record decides; it proves nothing. ADR 0007's fourteenth amendment and ADR
0039's fifth amendment point here.

## The question

A federated plan on node A has a leg over a source that node B owns. Which crates carry that leg
from A to B? How does the caller that A verified reach B, so that B runs the leg as that caller,
never as B's deployment, and refuses an anonymous caller?

Node A plans the question and runs the combine. Node B owns the source of one leg.

## The owner's answers

| Question                                         | Answer                                                                                                   |
| ------------------------------------------------ | -------------------------------------------------------------------------------------------------------- |
| Q1. What crosses the wire for a leg?             | A typed leg document. B parses it against its own bundle. The values travel as one Arrow parameter batch |
| Q2. Framing?                                     | One Arrow Flight `DoExchange` per leg                                                                    |
| Q3. Is B a worker under the 2026-10-12 decision? | Yes. A exchanges the caller's token at the operator's issuer for B's audience                            |
| Q4. Mutual TLS?                                  | Server-authenticated TLS now. Mutual TLS with certificate-bound tokens (RFC 8705) later                  |
| Q5. Which sources may B serve to a remote leg?   | Only `impersonation-at-source` sources that run legs                                                     |
| Q6. Which listener serves Flight on B?           | `sutura-http`'s listener, with HTTP/2 behind a default-off feature                                       |
| Q7. Multi-hop?                                   | One hop only                                                                                             |
| Names                                            | `sutura-exec-node` (the client crate), `sutura-flight` (the transport crate), source kind `node`         |

## Decision

1. **The leg port does not change.** A remote leg is a `Warehouse`. `sutura-app`'s leg runner
   already sends each leg to its source's `Warehouse` with `Executable::Leg`, after the credential
   and posture checks (`crates/sutura-app/src/federated/leg.rs:28-131`), through the runner
   (`:208-234`). So the credential per leg, the posture check, the deadline, the spend charge and
   the refusal mapping stay in `sutura-app` on A. No new port trait arrives.
2. **Two new crates.**
   - `sutura-exec-node` is the client on A. It implements `Warehouse` with
     `IMPERSONATION = PerSubjectCredential` (`crates/sutura-domain/src/source.rs:506`) and
     `EXECUTES_LEGS = true` (`crates/sutura-domain/src/warehouse.rs:362`). It sends only
     `Presented::SubjectToken` (`crates/sutura-domain/src/identity/credential.rs:259`) and refuses
     every other shape by name, as Cube's `deliverable` does
     (`crates/sutura-exec-cube/src/lib.rs:165`). It is behind a default-off feature on `sutura-cli`.
   - `sutura-flight` is the transport on B. It is an Arrow Flight service over `sutura-app`'s
     driving port. `Surface` gains one operation: run one remote leg through B's own pre-flight,
     spend charge and leg execution. It is a new variant of `Capability`, `RunRemoteLeg`
     (`crates/sutura-app/src/capability.rs:66`), added in #137 step 3, with its own scope literal
     `sutura:legs.run`, ordered after `AskMetric` and before `RunSql`. A token that grants only
     `sutura:metrics.ask` at B cannot send a leg. Only `sutura-flight` serves `RunRemoteLeg`: the
     agent and HTTP surfaces do not advertise it and remove it with `Permitted::without`
     (`crates/sutura-app/src/capability.rs:236`), so a caller at B that holds the scope gets no
     agent tool for it. This agrees with *Out of scope*: B's Flight endpoint serves sutura nodes
     only. Which tokens carry the new scope is the issuer's exchange policy. `sutura-http`
     carries the service opaquely, as it carries `/mcp`, behind leg 1, with one ungoverned-route
     row. `sutura-cli` pairs the two behind a default-off feature.
3. **A typed leg crosses the wire, never SQL text.** The domain holds one wire document for a leg,
   on the `Query` precedent: `Deserialize` with `deny_unknown_fields`. B parses it into a `LegPlan`
   against its own certified catalog, and never trusts the sender's leg for what a metric means. B
   refuses every leg that names a metric, table, column or metric term that B does not certify, or
   that omits a required filter of B's own definition of the metric
   (`crates/sutura-domain/src/measure.rs:383-389`). The refusal is a new variant
   `RemoteLegNotCertified { source }` of `sutura-domain`'s `RefusalReason`
   (`crates/sutura-domain/src/query.rs:237`), added in #137 step 3. A maps it to the same variant,
   naming its `node` source. So B judges a leg by B's own certified definition, and a leg that
   disagrees with it does not run. The values travel as one Arrow parameter batch. `PlanBindings`
   keeps no `Deserialize` derive, held by `check-serde-parse`, because it has a fallible
   constructor. `LegPlan` has none, so that gate does not read it: the compiler holds it while its
   fields `StatementTables` and `PlanBindings` have no `Deserialize`, and review holds the rest.
4. **One call per leg.** Arrow Flight `DoExchange`: the descriptor is the leg, the first message is
   the parameter batch, the reply is the result batches. B keeps no state between calls.
5. **The identity is an RFC 8693 exchange at the operator's issuer.** A `kind: node` source declares
   `delegation`. Its last hop's audience is B's resource identifier. A's
   `DeclaredPrincipalBroker` adds the source with `authenticating`
   (`crates/sutura-config/src/credentials/declared.rs:158`). So each request exchanges the
   caller's verified token and presents the result. A sends it as `authorization: Bearer` gRPC
   metadata over TLS. B runs leg 1 in `direct` mode and verifies the token as its own caller. B's
   own broker then runs B's source as that caller, through B's own broker declaration for that
   source. In this record, "B runs the leg as the caller" means "B runs the leg as the subject of
   the exchanged token". The issuer chooses that subject; A does not compare it with the caller A
   verified (see *Limits*).
6. **Server-authenticated TLS now.** A refuses an `http://` endpoint. Mutual TLS with
   certificate-bound tokens is a later decision, and it needs the operator's issuer to bind the
   exchanged token.
7. **B serves a remote leg only over an `impersonation-at-source` source that runs legs.** The
   check compares the posture variant, never the value (`.agents/skills/sutura/identity/SKILL.md`).
   A shared source at B is refused by name. So A's per-leg disclosure (ADR 0040) names a posture
   that is true at B.
8. **One listener.** B serves Flight on `sutura-http`'s listener. HTTP/2 is on only with the
   default-off feature. There is one place that arms leg 1.
9. **One hop.** B refuses a remote leg whose source at B is itself a `node` source.

## The refusals this adds

| Where      | Refused                                                                                                  | Why                                                                                                                                                             |
| ---------- | -------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A, load    | A `node` source with no `delegation`, with `delegation` on a shared entry, or with an `http://` endpoint | The `InvalidDelegation` set, reused (`crates/sutura-config/src/sources/placement.rs:667-688`). No caller's raw token is sent, and no token crosses in the clear |
| A, load    | A `delegation` while the inbound mode is not `direct`                                                    | Existing: `NotFitToServe::DelegationWithoutDirectInbound` (`crates/sutura-config/src/settings/posture.rs:200-216`)                                              |
| A, load    | The last hop's audience equals A's own resource identifier                                               | Such a token is valid at A too, so it is the caller's own token by another route                                                                                |
| A, request | An anonymous caller, or a caller with no assertion                                                       | Existing, at the broker (`crates/sutura-config/src/credentials/declared.rs:38-39`)                                                                              |
| A, request | An exchange that fails, or returns the wrong audience, the wrong type or an expired token                | Existing: `DelegationFailed`, `503 identity_unavailable`                                                                                                        |
| A, request | A `Presented` other than `SubjectToken`                                                                  | Decision 2                                                                                                                                                      |
| B, boot    | The Flight surface on, and the inbound mode not `direct`                                                 | B must verify the exchanged token as its own caller                                                                                                             |
| B, boot    | The Flight mount with a declared inbound identity that is not attached                                   | The router does not build, as for `/mcp` (`crates/sutura-http/src/router.rs:599-612`)                                                                           |
| B, request | A token that does not verify                                                                             | `401` before any handler                                                                                                                                        |
| B, request | A leg over a shared source, over a source that runs no legs, or over a `node` source                     | Decisions 7 and 9                                                                                                                                               |
| B, request | A leg with an uncertified metric, table, column or term, or with no required filter of B's definition    | Decision 3: `RemoteLegNotCertified`                                                                                                                             |

Each new variant is enrolled in `check-refusal-coverage`. Each refusal gets its own cell.

## What crosses between the nodes, and what does not

- The token travels only in gRPC metadata, as a `Secret`. It is never in a descriptor, a result, a
  log line or an error.
- A refusal crosses as a closed code, never as data-system text (ADR 0022). A maps it to its own
  predicates: source refused, result too large, deadline, working set. A `401` from B is an
  operator misconfiguration. It reaches the caller as `503 identity_unavailable`.
- A sets the gRPC deadline from what its `Deadline` has left. B opens its own `Deadline` at the
  earlier of that and its own request budget.
- A bounds what B returns through `Accumulating::push`, the only constructor of `ResultBatches`. B
  bounds its own leg.
- No row is stored at A or at B. `check-answer-path-caches` scans only `sutura-app` and
  `sutura-domain` today, so it gains the two new crates. Its limit applies there too: it reads one
  shape, a struct field or a `static` whose map value names an answer-path type, not a guarantee
  (`xtask/src/answer_path_cache.rs:24-37`).

## Limits

- **Flight `DoExchange` is untested.** #137 step 1 measured Flight SQL prepared-statement binding
  only: `Int64`, `Utf8` (NUMERIC as text) and `Date32`, with nulls, in one process over loopback.
  It ran no TLS, no authentication, no identity, no `DoGet` and no other type. Both framings carry
  the parameters as Arrow IPC, but that is reasoned, not run, for `DoExchange`.
- **The issuer is the operator's.** sutura assumes the operator configures a valid issuer and
  identity pool (`docs/where-identity-is-proven.md:142`). This record extends that assumption to
  the issuer's exchange policy. A reads the issued token's `aud` and `exp` without a signature
  check. That is a misconfiguration check, not a defence. B verifies the signature.
- **The subject at B is the issuer's, not A's.** A's exchange client checks the token type,
  `aud` and `exp` (`crates/sutura-http-client/src/delegation.rs:244-266`), not `sub` or `iss`. An
  exchange policy that answers with another subject makes B run the leg as that subject, while A's
  audit line and A's per-leg disclosure (ADR 0040) name the caller. Microsoft Entra ID's
  on-behalf-of grant (`delegation.rs:116-118`) issues a pairwise `sub` for each application. So,
  for the same person, B's `SubjectKey` (`crates/sutura-domain/src/identity/principal.rs:239-249`)
  differs from A's, and B's audit lines, B's spend ledger and a `switching` subject map at B use a
  different key from A's. That is the design of the grant. A strict `sub` equality would refuse
  every on-behalf-of exchange.
- **B's bundle can be another version than A's.** A's answer names A's definition digest
  (`crates/sutura-domain/src/pinned.rs:177-179`). B's check (decision 3) holds that the leg agrees
  with B's certified definition. It does not hold that the two bundles are one version.
- **"Short-lived" is held by the issuer only, so a captured token replays at B until it
  expires.** A refuses only a token that has already expired (`delegation.rs:265-266`). `direct`
  has no lifetime ceiling (`crates/sutura-config/src/inbound.rs:476-478`) and no `jti` store, so
  the issuer's lifetime is the window. TLS protects the token in transit. Bound
  tokens (decision 6) are what would close this.
- **B cannot tell node A from another holder of a token for B.** The token's client is recorded on
  the audit line and grants nothing.
- **A's exchange client credential is as strong as every caller whose token A holds.** With it, a
  token for B can be obtained for any such caller
  (`crates/sutura-domain/src/identity/delegation.rs:21-23`).
- **Two spend ledgers.** A's client answers `PreFlight::NotAsked`, so A charges nothing for the
  remote leg. B charges its own ledger. One subject can spend up to both ceilings.
- **One more issuer round trip per remote leg per request**, because nothing caches an exchanged
  token (`crates/sutura-domain/src/identity/delegation.rs:10-14`). Not measured.
- **The client blocks a thread.** `Warehouse::execute` is synchronous and the gRPC client is not.
  Whether the deadline still cancels the call there is not measured.
- **A new parser at a trust boundary.** B's leg parse gets a fuzz target.
- **The listener speaks HTTP/2 when the feature is on.** `sutura-http` is HTTP/1 only today and
  keeps `tonic` out of the default build (`Cargo.toml:467-476`). The feature adds `arrow-flight`,
  `tonic`, `h2` and `prost` to a build that asks for it. The lockfile cost is not measured.
- **The venue is in process.** Two nodes in one process, with the mock issuer and a loopback
  exchange fake. It can show what A sends, what B refuses and which subject B runs. It cannot show
  a real issuer's exchange policy or a real network. The multi-node machine tests of #828 are a
  different venue.

## Options considered

- **Forward the caller's own token** - not taken. The owner decision forbids it. A token valid at
  A and B lets any holder call B and skip A.
- **A node-signed subject assertion, verified at B against A's published key** - not taken. B holds
  only A's assertion, which the operator's issuer and pool do not trust, so B cannot run an
  impersonating source as the caller. B would also have to run `behind-gateway` for every caller,
  because a deployment has one inbound mode. A would become an issuer with its own signing key.
- **Mutual TLS now** - not taken yet. In-process mutual TLS at B tells B the peer holds a key its
  authority signed. It does not bind the token to A. B's listener has no client authentication
  today (`crates/sutura-http/src/tls.rs:389`).
- **SQL text through B's raw SQL tool, or in B's source dialect** - not taken. B would accept any
  statement from any holder of a token for B. The raw tool is off by default and charges no spend.
- **Flight SQL prepared statements** - not taken for a typed leg. Four to five calls per leg, and a
  handle that must carry its own state.
- **A second listener for Flight** - not taken. It needs a second place that arms leg 1, and one
  transport crate cannot depend on another.
- **The ADBC Flight SQL driver as the client** - not taken. ADBC connects a client to a database,
  not two sutura nodes. The driver is a second Go archive beside the BigQuery driver. Not measured
  in this record.
- **A Flight SQL server over a DataFusion session** - not taken. B's leg would skip `sutura-app`'s
  per-leg guards.
- **The client inside `sutura-exec-datafusion`** - not taken. The combiner would hold the
  credential, and an adapter would call another adapter.
- **A shared source behind B, with B's posture returned to A** - not taken. It needs a type change
  to `ExecutedAs`. A refusal keeps the disclosure true with no new type.
- **Chains of nodes** - not taken. Each hop would exchange again, and each hop adds the limits
  above.

## Out of scope

- Storing or forwarding rows.
- Who owns the combiner. A's `FederationCombiner` combines; B runs one leg.
- Elastic multi-node workers, their operator and warm capacity (#828). A worker reuses this
  identity rule.
- Generic Flight SQL clients and servers. B's Flight endpoint serves sutura nodes only.
- A served human caller venue. It is not built, by decision.

## Consequences

- A remote leg needs `security.inbound.mode: direct` on both nodes, and one more exchange client at
  the operator's issuer for each `node` source.
- #137 step 3 builds this in at least two PRs, so that `just causality` can build a base: first the
  domain's wire leg and B's parse, then the client, the transport and their cells.
- `docs/where-identity-is-proven.md` gains a row for the remote leg when its cell exists.
