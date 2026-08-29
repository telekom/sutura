---
title: A credential per leg, for the calling subject
description: What end-to-end impersonation concretely requires of BigQuery, PostgreSQL and Oracle, why the credential is minted per request and never falls back to a service identity, the signature of the CredentialBroker port, why one asker and one deadline are hoisted above N legs so two legs cannot disagree, exactly what that makes true and what it does not, how a deployment may mix impersonating and shared-service-user sources - a posture and an operator acknowledgement per source, checked where settings are parsed - which identity re-runs the anchors at boot and at every refresh, why the legs run one after another, what is refused rather than degraded, and the two-subject test that makes the invariant real. Oracle commits to the shared posture only and its impersonation is DEFERRED - the capability exists in the database and no Rust driver exposes it.
---

# A credential per leg, for the calling subject

Status: **accepted as a design, and nothing in it is built.** No line of this record describes code
in the workspace. It decides the shape of the identity path before the first adapter that needs one,
because both halves of that path are cheap to decide now and expensive to retrofit: the transport has
to learn a subject, and the execution port has to stop being able to run without one.

**Superseded in one part** by [the plan](0009-the-plan-from-one-source-to-many.md): sutura declares no
data sensitivity. What a person may see lives in the data catalog and in their own permissions at the
source, which is what reaching it as the subject is for, and a classification here would be a second
opinion about someone else's authorization. Everything about the credential, the chain, the mode and
the refusal survives. Section 5c was **rewritten** rather than annotated - the requirement drops from a
dataset to a source and becomes an operator acknowledgement - and 5a, 5d, 5e, 5g, the tests and the
table at the foot were brought into line with it, because a withdrawal banner over four parts that
still checked the withdrawn field was the worse half of a half-done revision.

**The vocabulary here is the one the other records use, and this record is the one that moved.** The
postures were spelled `SourceIdentity::{Impersonated, Shared}` in an earlier version of this section,
against `SharedServiceUser` and `ImpersonationAtSource` in
[pluggable by declaration](0011-pluggable-by-declaration.md), in
[the plan](0009-the-plan-from-one-source-to-many.md) and in `docs/implementation-plan.md` - three
files against one. They are renamed throughout, in prose and in every code block, rather than
reconciled with a note: a compatibility sentence between two spellings of one enum is a thing the
first implementer has to adjudicate, and the point of writing a record is that they do not have to.

**Two things this revision decided that earlier versions left silent**, both because silence in a
record of this kind resolves itself into the permissive answer:

- **What one `LegCredentials` value actually makes true.** It hoists the *asker* out of the legs, and
  a leg on a shared source deliberately does not execute as the asker - so "every leg runs as the same
  subject" was an overstatement of what the shape delivers. Part 4 now states the two claims that are
  true, encodes the per-leg effective posture in `Presented` as a third variant carrying no credential
  material at all, and says plainly what remains an operator obligation. `AGENTS.md`'s *Changing The
  Query Path Or The Tool Surface* row still carries the old sentence; narrowing it is a change to that
  file rather than to this one.
- **Which identity re-runs the anchors**, at boot and at every refresh
  [pluggable by declaration](0011-pluggable-by-declaration.md) adds a TTL for. Part 1 decides it, and
  says what it costs the reach - not the mechanism - of `AGENTS.md`'s "an unvalidated bundle is never
  served".

It supersedes nothing. It amends one sentence of
[a refusal carries a status](0005-a-refusal-carries-a-status.md) - the note that "the `403`s are not
a statement about a credential" - and *What is refused, and what is not a refusal at all* below says
how. It is the record
[federating across different data systems](0007-federating-across-different-data-systems.md) defers to:
that one decides per-source adapters rendered by `sutura-sql` and combined by DataFusion, states that
this shape "is what keeps per-subject execution reachable", and says the milestone "cannot honestly
ship without the credential port". This is that port. The two are read together and neither repeats
the other: architecture there, identity here.

## What is true today

Read this first. The rest is written in the future tense on purpose, and the reason is that every
identity claim this repository makes about itself is currently a design target. Each row was checked
against the tree at the commit this record was written on, not remembered.

| Claim | Where | What is actually there |
| --- | --- | --- |
| The deployment declares that it has no identity | `crates/sutura-config/src/security.rs` | `SecuritySettings::describes_identity()` is a `const fn` returning `false`. It is printed by the startup log so an operator cannot deploy believing otherwise |
| The bearer gate authenticates the deployment | `crates/sutura-http/src/middleware.rs` | `require_token` compares the presented token against the configured one in constant time and builds no principal. A deployment with no token configured - permitted on loopback outside production - passes every request through |
| Nothing on the request path verifies a signature, and nothing could | `crates/sutura-http/Cargo.toml`, `Cargo.lock` | The manifest names no cryptographic dependency. No JOSE, JWT or JWKS crate appears in `Cargo.lock` at all; `ring` resolves only under the default-off `tls` feature's rustls stack and under `rcgen`, which is a dev-dependency. The one cryptographic primitive in the serving path is `subtle`, for the constant-time comparison above |
| No subject reaches execution | `crates/sutura-domain/src/warehouse.rs` | `Warehouse::execute(&self, plan: &QueryPlan)`. There is no second parameter, and `&self` means an adapter's identity is a property of the adapter rather than of the call |
| No subject reaches the application | `crates/sutura-app/src/surface.rs` | `Surface::answer(&self, query: &Query)` |
| An anchor already executes with no identity at all, before any caller exists | `crates/sutura-app/src/lib.rs`, `crates/sutura-app/src/surface.rs` | `verify_and_validate` calls `verify_anchors`, which calls `check_one`, which calls `warehouse.execute(&plan)` - and `LocalService::start` calls `verify_and_validate` during composition. So the boot path is already a caller of the execution port, and part 1 cannot change that port without answering as whom it calls |
| One process, one connection, one OS identity | `crates/sutura-exec-duckdb/src/lib.rs` | `DuckDbWarehouse` holds a single `duckdb::Connection`, opened in `open` or `in_memory` and kept for the value's life |
| A data system is not even configurable | `crates/sutura-config/src/defaults.yaml` | There is no `sources:` key. Which catalog and which data system are compile-time decisions in `sutura-cli` |
| The domain reads no clock | `sutura-domain`, `sutura-app` | `SystemTime`, `Instant` and `std::time` appear in neither crate. `TimeRange::new` takes two explicit `Date`s; nothing in the interior asks what time it is |
| The refusal enum already says so | `crates/sutura-domain/src/query.rs` | `RefusalReason::SourceUnavailable`'s own documentation states that what raises it is a name comparison and not an identity check, and that the identity half "is a design target and not a control" |

So `AGENTS.md`'s opening line - "we support e2e impersonation" - describes an intention, and its
*Changing The Query Path Or The Tool Surface* table is the honest version: a test that two subjects
get different rows "does not exist and cannot, until a credential exists per leg". This record is
about making that sentence obsolete.

## What impersonation requires, per data system

Federation is wanted for three data systems, and the routes on the table differ in exactly one
respect: whether per-subject identity stays reachable. That trade cannot be made without knowing what
each system concretely requires, so this section is the evidence and the rest is the conclusion.

**The labels.** Each claim is *verified* against the vendor's current documentation, *reasoned*, or
*needs reproducing* - the last where the documentation does not settle it and the honest answer is
that somebody has to run it, which is this repository's rule for external behaviour applied to a
design document.

### BigQuery

**The mechanism is a token, and the obvious mechanism is a dead end.**

*Verified:* service-account impersonation cannot produce an end-user identity. The IAM Service Account
Credentials API's `generateAccessToken` names its target by the resource pattern
`projects/-/serviceAccounts/{ACCOUNT_EMAIL_OR_UNIQUEID}`, so what it mints is always a service
account - useful for a service becoming another service, and incapable of making a query run as a
person. It needs the token-creator role on the target, and its lifetime is one hour by default and at
most, reaching twelve only for an account named in an organization policy that allows the extension.

*Verified:* the path that works is **workforce identity federation with an
[RFC 8693](https://www.rfc-editor.org/rfc/rfc8693.html) token exchange**, and BigQuery is generally
available for it with no documented query-path limitations. The gateway is registered as an OIDC
provider on a workforce pool; the caller's ID token is posted to the Security Token Service with the
token-exchange grant type, a subject token type of `urn:ietf:params:oauth:token-type:id_token` and a
billing project in the options. The token that comes back has the **person** as its principal, as
`principal://iam.googleapis.com/locations/global/workforcePools/<pool>/subject/<subject>`, with no
service account in the chain; grants are written against that identifier and its `principalSet://`
group and attribute forms. Lifetime is the pool's session duration - fifteen minutes to twelve hours,
one hour by default.

*Verified, and it constrains the port:* the exchange request body has **no `actor_token` field**. RFC
8693 distinguishes *impersonation*, where the acting party becomes indistinguishable from the subject,
from *delegation*, where both identities are expressed and the issued token carries an `act` claim.
Only the first is available here, so the data system's audit sees the person and cannot see that
sutura acted for them: **recording the principal chain is sutura's job and cannot be delegated to the
data system.** That is a requirement on the audit sink, not a detail.

*Verified:* there is no in-session identity switch. The GoogleSQL data control language is `GRANT` and
`REVOKE` and nothing else, and `SESSION_USER()` reads the identity without changing it, returning the
`principal://` form for a federated user.

**What the system enforces, so what sutura gets for free.** *Verified:* row access policies filter
against the authenticated principal, and a principal in no grantee list sees no rows. Column access
control via policy tags **errors** rather than nulling, naming the columns. Dynamic data masking
returns a **masked value** with the masked-reader role, and denies permission with neither role.
Masking applies on top of row-level security. An authorized view bypasses table and dataset IAM on its
source and does **not** bypass either fine-grained control.

*Needs reproducing:* the outcome for a principal holding table read permission that matches no row
access policy. The documentation says "cannot see any rows" in three places and describes an
access-denied error in a fourth, for the neighbouring case of a missing table permission. Zero rows is
the weight of the evidence, and not a sentence to write as a guarantee.

**Pooling is the easy case here, and one thing is not.** *Verified:* the credential is an
`Authorization: Bearer` header on each REST call. There is no connect handshake, no session identity
on a socket, and therefore no dirty-connection failure mode. A BigQuery *session* is a value in a
request body, not state a connection holds.

*Verified:* the hazard moves to caching instead, and it is silent. A process-wide access-token cache
converts every request into the cached subject's request, and nothing in BigQuery objects, because the
token is the identity every check above reads. Cached results are per-user and per-project, with
cross-user reads available on higher editions - though a table under row-level security is not cached
at all, which removes the case that would matter most.

*Needs reproducing, and it matters for this repository specifically:* the Storage Read API splits its
permissions, requiring `bigquery.readsessions.create` to create a read session and only
`bigquery.readsessions.getData` on the stream to read its rows, with the filter and snapshot fixed by
the creating identity. That reads as a bearer capability over one subject's filtered rows, held by a
stream name - and the Storage Read API is the Arrow path this repository's own provenance design
points at. Nothing may pass a stream name across subjects, and this is the one place where "the
credential is per request" is not the whole story.

**Token exchange:** RFC 8693, with [RFC 8707](https://www.rfc-editor.org/rfc/rfc8707.html) resource
indicators for audience restriction. *Verified* trap for anyone implementing it: the caller's ID
token must carry the workforce provider's configured **client ID** in `aud`, which is not the
`audience` value sent to the exchange endpoint - the opposite convention from workload pools.
*Verified* dead end: credential access boundaries, the mechanism for downscoping a broad token, are
available for object storage only, so a broad token cannot be narrowed per question after the fact.

### PostgreSQL

Read against the current stable release, 18.

**Two mechanisms, and one of them is disqualified by its own privilege requirement.**

*Verified:* `SET ROLE` changes `current_user` and leaves `session_user` alone. What permits it is the
`SET` option on a role grant, which is a PostgreSQL 16 addition and **defaults to true** - not the
`ADMIN` option, which controls onward granting instead. So the service's login role needs
`GRANT <subject_role> TO <service_role>` for every subject it may become - and *verified:* the
least-authority spelling is `WITH INHERIT FALSE, SET TRUE`, which lets the service *drop* into a
subject's privileges rather than accumulate them, so the service role does not hold the union of
every subject's rights while merely sitting there.

*Verified:* `SET SESSION AUTHORIZATION` changes both identifiers, and the privilege test is not
"superuser now" but "**the initially authenticated user** has the superuser privilege". A service
using it must therefore connect as a superuser, which for a read-only semantic layer is disqualifying
on its own, and this record treats it as unavailable rather than as an option with caveats.

*Verified, and it is the hard limit:* **neither form can be made irreversible.** `SET ROLE NONE`,
`RESET ROLE` and `RESET SESSION AUTHORIZATION` are each documented as executable by any user. There
is no way to bind a *borrowed* connection to a subject for the duration of a query. The only binding
PostgreSQL offers is authenticating as the subject, where a non-superuser's `session_user` is then
immutable for the connection's life.

*Verified:* PostgreSQL 18 added native OAuth: an `oauth` method in the host-based authentication
configuration, over SASL `OAUTHBEARER` as defined by
[RFC 7628](https://www.rfc-editor.org/rfc/rfc7628.html), with `issuer` and `scope` required per entry.
Three things about it are load-bearing and none is a detail. **It ships no validator**: the
`oauth_validator_libraries` setting is empty by default and an empty value refuses every OAuth
connection, so the deployment supplies a C shared library loaded into the server process - whose own
documentation warns that a malfunctioning validator is worse than no authentication. The validator,
not the server, is what checks issuer, audience and validity. And a client that already holds a token
can inject it rather than run a browser flow, through libpq's authentication-data hook.

**What the system enforces.** *Verified:* row-level security policies are evaluated against
`current_user`, so `SET ROLE` is sufficient and session authorization is not required. Enabling row
security with no matching policy is **default deny**. Two exemptions matter: a role holding the
`BYPASSRLS` attribute bypasses everything, and **the table owner bypasses its own table's policies
unless the table is declared `FORCE ROW LEVEL SECURITY`**. Column privileges refuse at table
granularity - the error is `permission denied for table`, naming no column, which is the opposite of
BigQuery's behaviour and worth knowing before writing a diagnostic that assumes otherwise.

*Verified, and it is the trap most likely to silently void this entire design:* **a view's default is
to read its base tables as the view's owner, policies included.** A plain view over a table with row
security applies *the view owner's* policies, not the caller's. `security_invoker = true`, added in
PostgreSQL 15, is what makes a view read as the invoking user. A semantic layer that resolves metrics
through views - which is the normal way to model one - defeats per-subject row security by default
unless every such view sets it. That is a candidate for a gate rather than a note: sutura's catalog
names the relations it reads, so it is in a position to refuse a bundle whose relations are views
without `security_invoker`.

**Pooling, where this design would die, and the evidence is unusually direct.**

*Verified:* there is no server-side way to observe a leaked role. `pg_stat_activity` reports the
login user and is never updated when `SET ROLE` runs, so detection has to be in-session - which
confirms the check-in rule in part 8: ask the connection who it is.

*Verified, and each of these is a way to get it wrong:*

- **`RESET ALL` does not reset the role.** Both the `role` and `session_authorization` settings carry
  the server's no-reset flag, documented nowhere in the manual. `DISCARD ALL` does clear both, being
  defined as running `SET SESSION AUTHORIZATION DEFAULT` first, and cannot run inside a transaction
  block. *Needs reproducing before being written anywhere but here*, since the evidence is the
  server's source - but the direction is unambiguous, and a pool checking in with `RESET ALL` leaks.
- **`RESET ROLE` returns to the connection-time role setting, not to `session_user`**, so it
  re-impersonates if the service ever set a role through connection options. `SET ROLE NONE`
  unconditionally returns to the login identity, and is what a check-in path should use.
- **`SET LOCAL ROLE` outside a transaction block emits a warning and does nothing.** It fails *open*,
  into the service's full privileges. The transaction-scoped form needs an explicit transaction as a
  checked precondition plus an assertion on `current_user` afterwards.
- A rolled-back transaction reverts a `SET ROLE`, as does rolling back to an earlier savepoint.

*Verified, and this is the finding that decides the posture:* **"one long-lived session, many
subjects" is a documented vulnerability class in PostgreSQL, with four CVEs and a track record
spanning a decade.** Row-security policy decisions are cached with the query plan, and the cache has
repeatedly failed to notice that the current role changed:

All four are linked to the project's own advisory page rather than to a vulnerability aggregator, and
the reason is the same reason this record labels its evidence: the upstream advisory is the party that
decides what is affected and what fixes it.
[`CVE-2016-2193`](https://www.postgresql.org/support/security/CVE-2016-2193/) - "Plan cache might use
wrong role context for RLS policy" - was a cached plan generated for one role reused under another,
applying the wrong policy set, and the upstream commit names the triggering pattern as a common user
and query planned once and reused across multiple `SET ROLE`s.
[`CVE-2023-2455`](https://www.postgresql.org/support/security/CVE-2023-2455/) was policies
disregarding user ID changes after inlining;
[`CVE-2024-10976`](https://www.postgresql.org/support/security/CVE-2024-10976/) the same below
subqueries. And [`CVE-2026-14666`](https://www.postgresql.org/support/security/CVE-2026-14666/) -
"PostgreSQL row security caching disregards role modifications" - published 2026-08-13 and fixed in
18.5, 17.11, 16.15, 15.19 and 14.24, is row security caching disregarding role modifications, where
stale policies continue "until some other event invalidates the cache or connection termination ends
the session".

Two conclusions follow, and neither is a matter of taste. A pooled connection **extends the blast
radius** of this class, because the documented persistence condition is the session's lifetime. And
the pattern the oldest of them names is precisely "shared pool plus `SET ROLE`", so an architecture
reaching for it as the scalable default is choosing the shape with the incident history. It remains
usable; it is not the shape to treat as obviously safe.

**Token exchange, and the two options with their trust stated.**

*Option A - map a validated claim to a role and `SET ROLE` on a connection the service authenticated.*
Available on every supported version. The service becomes the sole verifier of the token and holds a
credential that can become **every** subject role, so a bug in the claim-to-role mapping is a full
cross-subject read with nothing in the database constraining it. *Verified:* `system_user` records the
*service's* authenticated identity, so the database's audit cannot attribute the query to the person -
the second system here where the principal chain has to be sutura's to record. Impersonated roles also
do not receive their own `ALTER ROLE` settings, so per-role hardening silently does not apply.

*Option B - authenticate as the subject with the token.* PostgreSQL 18, the `oauth` method, a supplied
validator. It puts the subject in `system_user`, makes `session_user` immutable for the connection, and
takes the service out of the trust path for identity. It costs a bespoke security-critical C library in
the server, a per-subject connection budget, and no pooler - *verified:* the common pooler's
authentication methods do not include OAuth, so the token cannot terminate there. *Reasoned, neither
stated nor precluded by the documentation:* the caller's front-door token is audience-scoped to sutura
rather than to the database and the validator is told to check audience, so an audience-correct token
must be obtained somehow, and RFC 8693 is the standards-based way - an inference from the audience
requirement rather than a documented PostgreSQL position.

*Verified, and it applies to both:* nothing re-checks a token mid-session, so a session outlives the
token that opened it - the same conclusion the expiry discussion reaches from the other direction, and
one more reason a long-lived session is the wrong shape here.

### Oracle

Read against Oracle AI Database 26ai, the current release, with 19c noted where a claim is
release-scoped.

**Proxy authentication, and it is the best-shaped classic mechanism of the three.**

*Verified:* the grant is `ALTER USER <target> GRANT CONNECT THROUGH <proxy>` with
`WITH ROLE <r>`, `WITH ROLE ALL EXCEPT <r>` or `WITH NO ROLES`, and optionally
`AUTHENTICATION REQUIRED` - the older `AUTHENTICATED USING PASSWORD` spelling is deprecated.
`WITH NO ROLES` "prohibits the proxy from activating any of that user's roles after connecting, even
the secure roles", so least authority is expressible in the grant itself, which neither other system
offers at this granularity. The default is closed - "by default, the middle tier cannot create
connections for any client. The permission must be granted for each user" - and the grants are
auditable in `PROXY_USERS`. *Verified:* the feature "is supported in three tiers only" and does not
chain across several middle tiers, which constrains any topology that wanted a gateway to relay it.

*Verified, and it is the scaling constraint:* the target is a database user. `GRANT CONNECT THROUGH`
is a clause of `ALTER USER`, both sides appear as user names in `PROXY_USERS`, and for a federated
identity the documentation is explicit that "a separate database schema for the IAM user to proxy to
must also be available" and that the external user "cannot be referenced in the command". So classic
proxy authentication needs an account per subject. *Verified:* shared-schema mapping relieves the
count, mapping many external users onto one global schema - but *reasoned:* a `GRANT CONNECT THROUGH`
from a shared schema then covers every user mapped to it, trading account count for grant
granularity. Under a shared mapping the person is no longer in `SESSION_USER`; the identity survives
in `AUTHENTICATED_IDENTITY` and `ENTERPRISE_IDENTITY` instead.

*Verified:* `SYS_CONTEXT('USERENV', 'SESSION_USER')` is the **target** user and `PROXY_USER` is "the
database user who opened the current session on behalf of `SESSION_USER`". **So Oracle records the
principal chain in the session itself** - the delegation BigQuery's exchange endpoint cannot express
and that PostgreSQL loses under its option A - and it is usable defensively, since a policy can
require `PROXY_USER` to be the expected middle tier and deny otherwise.

*Verified, and it is the limit on that claim:* under a username proxy `AUTHENTICATED_IDENTITY`
returns the **proxy's** own database user name, and the caller's token is not carried into the
session. What the database records is "this account, via that middle tier"; it does not record that
the person authenticated, because nothing proved that to the database. The chain is real, and the
attestation behind its first link is still sutura's word.

*Verified, and it must be named to be dismissed:* `DBMS_SESSION.SET_IDENTIFIER` and the
`CLIENT_IDENTIFIER` it sets are not authentication. The attribute is for users "known to an
application but unknown to the database", it "can capture any value", the procedure "is executable by
`PUBLIC`", and applications may "reset the client identifier and thus reuse the session for a
different user". Using it as the identity would be trusting a value we set ourselves.

**What the system enforces.** *Verified:* Virtual Private Database adds a predicate to statements
against a protected object, and its policy function receives only the schema and object names - **no
identity argument at all** - so identity is read from `SYS_CONTEXT`. *Verified:* proxy authentication
is explicitly supported alongside it, with the documentation's own example checking `PROXY_USER` to
choose a policy group or deny access. *Reasoned, not verified:* that the policy therefore evaluates as
the target user. It follows from `SESSION_USER` being the target, and no single sentence states it, so
it is on the list to reproduce rather than assert.

*Verified licensing fact, and the only one obtained:* `DBMS_RLS` - Virtual Private Database - "is
available with the Enterprise Edition only". Whether Label Security, Real Application Security,
redaction or the 26ai framework below need separately licensed options is **not verified and not
asserted here**. It is a procurement question with a real answer, and an Oracle adapter should not be
planned without it.

**Pooling: Oracle can re-identify a live connection, and there are two sharp caveats plus one
foreclosure.**

*Verified:* a proxy session is opened on an **existing authenticated connection** - the driver asks
the server to create a session for the target, the server returns a session id, and the driver sends a
session-switch command. The physical connection is reused, the drivers "permanently switch to the new
session", and it is closed by closing the connection with the proxy-session flag. The pool's borrow
builder exposes proxy properties. So part 8's third posture is properly available here.

*Verified, and this is where a naive implementation breaks:* closing a proxy session "automatically
closes every SQL Statement created by the proxy connection, during the proxy session **or prior to**
the proxy session", which empties the statement cache a pool exists to keep; and a proxy connection
closed without applying the connection attributes "is returned to the connection cache for reuse, but
cannot be retrieved" - a leaked connection rather than a leaked identity, which under load is an
outage rather than a breach.

*Verified, and it forecloses a combination somebody will reach for:* **resident connection pooling and
per-borrow proxy switching do not combine.** "Creating multiple sessions on a DRCP server for session
switching or for dual session proxy" is among the operations that cannot be performed on a pooled
server, and attempting it "may result in the following error: `ORA-56609`". Worse for isolation, proxy
sessions on that pool "are shared among applications that are connected to the same schema", with a
shared default connection class. So it is resident pooling *or* per-subject sessions, and this design
needs the second.

*Verified:* the documented hygiene lever is a service attribute - `RESET_STATE` at `LEVEL1` on 26ai
clears session state between requests, because otherwise "cursors in FETCH status, and session state
set on that session remain in place unless an action is taken to clear them". On 19c the equivalents
are manual.

**Token exchange, three paths, and the third is aimed at this product.**

*Path (a):* map a validated claim to a user and open a proxy session. Available on every supported
release, keeps the chain in `PROXY_USER`, and is better than PostgreSQL's option A because the
database can see and audit the delegation. The trust in sutura is the same; the compensating control
is the DDL, where `WITH NO ROLES` and per-user grants bound what the service can do.

*Path (b):* pass a token to the database. *Verified:* the driver settings exist - a token
authentication mode, a token file holding an RFC 7519 JWT, TLS required - and **the database** verifies
it with a public key fetched from the authentication service, then maps group claims to roles at
session creation. It composes with proxy authentication. *Verified limits:* the documented server-side
issuers are two named identity services, one supported on-premises from 19.18 and on 26ai but not 21c
and the other cloud-only, and **a generic third-party OIDC issuer is not documented as accepted for a
driver session.** For a deployment whose identity arrives from its own gateway, that is the gap to
reproduce before designing on it.

*Path (c), and it earns its paragraph:* 26ai adds a database-enforced data authorization framework
aimed at this problem. *Verified:* its end users "do not own database schemas or objects"; access comes
from data grants carrying predicates over an end-user context; and in its mandatory mode "access is
denied even if the user possesses database object or system privileges". Its session model is **two
tokens presented together** - the end-user token and a database-scoped on-behalf-of token - both
validated by the database for trust, **audience** and validity, where "establishing a valid session
requires the application to present three elements simultaneously". Under that flow the
database-scoped token "carries the original user's identity (`sub` claim) but is addressed to the
database audience", and roles come from the user's own claims: "the application itself does not define
or control the roles". Its motivation section names agents that "inspect schemas and execute SQL
directly, bypassing application protections entirely" - this product's threat model, written by the
vendor. Its pooling story is the one this record wants: on reuse "the database seamlessly replaces the
previous security context with the new one", and on release it "is detached".

So path (c) removes both the per-subject account and the blind trust, because the database validates
the end-user token itself rather than taking sutura's word. It costs 26ai, the same
two-identity-service constraint as (b), and an audience configured on the database. *Not verified:* its
on-premises deployment matrix and its exact driver method names.

### Oracle commits to the shared posture only, and the impersonation decision is DEFERRED

**Decided: an Oracle source declares `SharedServiceUser`. It does not offer `ImpersonationAtSource`, and this record does not
choose between the two mechanisms that could.** The deferral is explicit rather than implied, because
the sections above work out both mechanisms in detail and a reader would otherwise reasonably conclude
that one of them is the plan.

**Why, and it is not an Oracle limitation.** Everything above about token authentication holds: the
database validates an externally-issued token, `AUTHENTICATED_IDENTITY` carries the person, global roles
activate from the token's claims, and the unified audit trail records the individual in
`EXTERNAL_USERID`. The capability is present in Oracle and in the C interface below it, where
`dpiCommonCreateParams.accessToken` carries the token. **This workspace has no Oracle driver at all** -
`Cargo.lock` on this branch contains no `oracle`, no `odpic-sys` and no Oracle client of any kind, and
an earlier version of this section said otherwise. That was an invented dependency in a record whose
whole argument is which claims are verified and which are reasoned, so it is corrected rather than
softened: what follows is a reading of UPSTREAM crates, at the versions named, not a statement about
anything this workspace links.

**The gap is a Rust one, and it is a missing safe wrapper rather than a missing capability.** *Verified
mechanically rather than from documentation:* the de-facto `oracle` crate's connection builder has no
token method and its `connection.rs` on master contains no case-insensitive match for `token` or
`oauth`, so `accessToken` stays null. *Verified:* Oracle's **own** official Rust driver reports token
authentication as unsupported in its feature matrix, carries an unreferenced token-mode constant in its
source, and its maintainer states on the open issue that the feature "is on the list of items to
implement but it will take some time." No production-viable Rust crate exposes the path.

**So the posture is shared, and the mechanism that enforces it already exists.** Part 5b's second,
orthogonal fact is whether the *adapter* can carry a per-subject credential at all - a property of code
rather than of configuration - and the cross-check is a startup refusal: `Impersonated` configured
against an adapter with no support cannot be deployed. **The Oracle adapter declares no support, so a
deployment that configures it as impersonated does not boot.** Nothing new is needed; this is that check
meeting its first real case.

**What follows for a multi-user deployment, stated plainly because it is the consequence somebody has to
act on.** In multi-user mode a `SharedServiceUser` source needs the per-source operator acknowledgement of part 5c,
naming the reason. And since sutura declares no data sensitivity and cannot see which dataset on a
source is critical, **keeping critical data off an Oracle source in multi-user mode is an operator
obligation, not something this system checks.** That is the same limit part 5c already states in
general; Oracle is the first source where it binds in practice rather than in principle.

**Nothing above is deleted, and that is deliberate.** The token-authentication and proxy-authentication
findings are the **input to the deferred decision**, not stale material: the day the driver gap closes,
the choice is between them and the research is already done. A section that vanished would leave the
next reader re-deriving it - the same reasoning that kept part 5c's derived-upward argument after its
field was withdrawn.

**What would close the deferral, in preference order:**

1. **Contribute the token wrapper upstream.** The `odpic-sys` crate exposes the field, so the missing
   piece is a safe wrapper over a binding that already exists - small, and the only option that reaches
   real token authentication without a fork. It benefits every other consumer of that crate, which is
   the right shape for a dependency gap. **Note this is upstream work on a crate we do not currently
   depend on**, so it is a contribution rather than a change here.
2. **Oracle's official Rust driver implements it.** Indefinite, and outside our control.
3. **Proxy authentication**, which works with today's drivers and is fully worked out above - carrying
   its own recorded limit that under a username proxy the caller's token never enters the database.
4. **A REST transport instead of a native driver, which is where
   [federating across different data systems](0007-federating-across-different-data-systems.md) already
   points.** THE TRANSPORT IS NOT THIS RECORD'S TO COMMIT, and an earlier version of this section said
   "the committed transport is the native driver", which contradicted an accepted record. 0007 owns
   transport and its argument is the stronger one: there is no Oracle driver this workspace can take,
   because every Rust option wraps Oracle's own client library, nixpkgs cannot supply it freely, so there
   is no analogue of the single path `nix/duckdb.nix` gives DuckDB and **`just validate` could not build
   it** - which is the only thing that counts as verified here. 0007 therefore adopts Arrow Flight SQL
   uniformly, with the proprietary client living in a gateway process outside our artifact.

   **So this is a second, independent blocker on Oracle impersonation, and it is the harder one.** The
   wrapper gap above is a missing safe binding somebody could contribute; this one says a native Oracle
   driver does not build in our sandbox at all. Whether Oracle's REST service or Flight SQL is the route,
   both put the proprietary client outside the artifact, which is 0007's position rather than a
   deviation from it. Two things would decide it, and neither is known: whether the REST layer propagates the end
   user's identity into the database session so row-level policies apply as that person - if it pools as
   a fixed schema user it is the shared posture with extra steps and buys nothing - and what it costs per
   query against the deadline and the working-set bound, row-by-row over HTTP rather than a native
   protocol.

**And one option is ruled out rather than deferred.** The Oracle Net `TOKEN_AUTH` / `TOKEN_LOCATION`
parameters make the client library do the token work with no driver API at all, and they read the token
**from a file on disk**. Per-subject impersonation would mean writing each caller's bearer token to the
filesystem, which is precisely what *a credential does not travel through a path* forbids, for exactly
the case the rule exists to cover. `TOKEN_LOCATION` is also per-descriptor, so per-subject tokens need a
distinct connect string each and defeat connection reuse, and the path requires the thick client, which
is glibc-only and cuts against the musl release story. **Not a fallback. A dead end, recorded so nobody
finds it and mistakes it for one.**

#### Why the option set is short: presentation versus delegation

Oracle supports many authentication methods, and the deferred decision has only three candidates. That
looks like an oversight until the discriminator is stated, so it is stated here to stop each method being
re-argued in turn:

> **A method that authenticates a secret the USER holds cannot impersonate from a middle tier. Only a
> method shaped like delegation can.**

| Method | Delegation-shaped | Why |
| --- | --- | --- |
| Token, with an on-behalf-of exchange | **yes** | The middle tier holds a token minted *for the user*, and never sees the user's own secret |
| Proxy authentication | **yes** | The middle tier authenticates as itself and asserts the target user; the database records both |
| Centrally managed users, password | no | The database validates the user's directory password, so the middle tier would have to hold it |
| RADIUS | no | Authenticates a credential the user holds - a password, a one-time code - and has no delegation concept. Also gated behind a separately licensed option, which makes it worse rather than better |
| Certificate / PKI | no | Requires the user's private key |
| Centrally managed users with Kerberos constrained delegation | *possibly* | Genuinely delegation-shaped, and the only directory-based candidate. Needs the service to hold a directory identity with delegation rights granted administratively, and almost certainly meets the same Rust driver gap from the other side: a workspace with no wrapper for a token field is unlikely to have one for a GSSAPI context. Heavier prerequisites, same blocker |

**One practical point that outlives the deferral.** Centrally managed users and token authentication share
the same mapping machinery: both are `IDENTIFIED GLOBALLY`, both offer an exclusive schema per person or a
shared schema with directory-group-to-global-role mapping. So a deployment that already maps its directory
into Oracle schemas has **already done the mapping work the token path needs** - only the issuer string
changes. That transfers, and it is the reason to know what an estate runs even while this decision is
deferred.

*Not established:* whether centrally managed users and `IDENTITY_PROVIDER_TYPE` may be configured on one
database at once, or whether enabling one excludes the other. It does not block anything today and is
listed so nobody assumes either answer.

#### Two Oracle-side features that look like they close this and do not

Both come up naturally when reading Oracle's own material, so both are recorded rather than left for the
next person to chase:

- **The cloud identity service, in its current identity-domains form.** This *is* the `OCI_IAM` issuer
  named above - not a third path beside it. It changes nothing here for two reasons: the blocker is a
  missing Rust wrapper rather than a missing issuer, and *verified:* that issuer is supported on the
  managed and cloud-at-customer database services only, **not on-premises**, so where an on-premises
  database is in scope it narrows the option set rather than widening it. Its tokens are also
  proof-of-possession with a private key rather than bearer, which is a different shape for the
  credential port than the directory-issuer path.
- **Anything else on the authentication list.** See the delegation table above. The discriminator is
  whether the middle tier can act without holding the user's own secret, and it disqualifies most of the
  list on shape, before any question of driver support arises.

**The general form of this, worth carrying beyond Oracle:** when a capability is present in a data system
and absent from its Rust client, **no amount of further capability in the data system closes the gap.**
Read the layer that is missing the wiring, not the layer that already has the feature. Three separate
searches of Oracle's surface area reached the same wrapper, which is the evidence for stating it once
rather than discovering it again per source.

### The three side by side

| | BigQuery | PostgreSQL | Oracle |
| --- | --- | --- | --- |
| What reaches an end user | Workforce identity federation, RFC 8693 exchange | (A) `SET ROLE` on a service connection, or (B) `oauth` as the subject, 18+ | (a) proxy authentication, any release; (b) a token, 19.18+ or 26ai, two named issuers; (c) the 26ai authorization framework |
| An account per subject | No - federated `principal://` identifiers | Yes - a role per subject | Yes under (a) and (b), relieved by a shared-schema mapping; no under (c) |
| Identity is bound to | each request | the session | the session, switchable in place |
| Re-identify a live connection | no connection identity to change | yes, and always revertible by anyone | yes, with a real open and close |
| Principal chain in the data system | no - no `actor_token` | no under (A), yes under (B) | **yes** - `PROXY_USER` beside `SESSION_USER` |
| Enforced for us | row access policies, policy tags, masking | row-level security, column privileges | Virtual Private Database (Enterprise Edition), roles bounded per grant |
| The pooling hazard | a cached token, silently | a connection returned dirty; a plan-cache CVE class | a check-in that empties the statement cache, and resident pooling that will not switch at all |

The row that decides the port's shape is the third: identity is per-request on one system and
per-session on two, so the credential cannot be a property of a long-lived adapter. It has to arrive
with the call.

## Decision

Eight parts. The first three are shapes, the fourth is the only one that turns an intention into a
mechanism, and the last four are what has to be declared, refused, carried and cleaned up. Part 1
carries two decisions rather than one, because the execution port has two callers and only one of them
is a question: a request, and the boot path that re-runs the anchors.

### 1. A subject on the execution leg, and never a fallback

**A credential is minted per request, for the calling subject, for each leg of the plan. A leg that
cannot run as the subject is refused. There is no service-identity fallback, and its absence is
structural rather than a rule somebody follows.**

`Warehouse::execute` gains a parameter and loses the ability to be called without one:

```rust
fn dry_run(&self, plan: &QueryPlan, as_: &Presented) -> Result<PreFlight, Self::Error> {
    Ok(PreFlight::NotAsked)
}
fn execute(&self, plan: &QueryPlan, as_: &Presented) -> Result<RowSet, Self::Error>;
```

`dry_run` takes it too, and not for symmetry: a pre-flight asks "would this be accepted", and the
answer depends on who is asking. Under the process identity it would report a plan as executable that
the subject may not execute, or prepare a statement against tables the subject cannot see. The check
has to be asked as the same principal as the question, or it answers a different question.

**Which is why `dry_run`'s return type changes with its parameter, and the default body stays.**
Today's default is `Ok(())`, and today that is defensible: with nothing to be wrong about, "nothing
went wrong" is honest. With a subject in the signature it stops being honest, because `Ok(())` from an
adapter that did not look is indistinguishable from `Ok(())` from an adapter that asked the data system
and was told yes - so a defaulted pre-flight would read as "this subject may run this plan" for every
adapter that declined to implement one. The fix is the shape this repository already uses for the row
cap, where `row_limit()` is `max_rows + 1` so a result *at* the cap is distinguishable from one cut off
*by* it:

```rust
/// What a pre-flight established. `NotAsked` is not `Accepted`, and no caller can read it as one.
pub enum PreFlight {
    /// The adapter did not ask. The default, and the honest answer for an adapter where checking
    /// costs what running costs.
    NotAsked,
    /// The data system was asked, as this subject, and accepted the plan.
    Accepted,
}
```

The default stays because the reason for it stands - `Warehouse`'s own documentation says an
in-process engine cannot make the trade, since checking means building the logical plan and running
the analyzer, which is most of executing it - and because a required `dry_run` would have such an
adapter implement a stub that lies. What changes is that the lie is no longer available: an adapter
that does not ask returns the variant that says so, and a caller that wanted authorization from a
pre-flight has to match on `Accepted` and get `NotAsked` instead. **The limit, stated with it:**
`Accepted` is still the data system's opinion at pre-flight time and not a guarantee about `execute`,
so it is worth a round trip and is not an authorization decision. Nothing in the plan path may treat
it as one, and there is no mechanism that would stop it - `answer` calling `dry_run` and skipping a
check on the strength of `Accepted` is a review question.

That is the whole mechanism for "no downgrade". Today's signature *is* the fallback: an adapter with
no credential parameter runs as whatever the process is, and nothing anywhere had to decide that.
After the change there is no code path that executes without something a broker produced, so
"downgrading to the service identity" is not a mistake anyone can make quietly - it is a signature
that does not exist. Every current adapter fails to compile until it says what it does with the
parameter, `sutura-exec-datafusion` included, and part 5 is what the file-backed case has to say.

[Concepts](../concepts.md#principal-subject-and-running-as-the-caller) already fixes the vocabulary -
the **principal** is who is asking, the **subject** is the identity the data system must see - and it
names the gap this part closes: "nothing here would catch a future adapter that fell back to a service
account". A signature with no credential parameter is what makes that adapter possible; requiring one
catches it at compile time, before it is written.

The reason to be this blunt is that the failure is silent and reads as success. A leg that answers
under the service's own identity turns "this subject may not see these rows" into "here are the
rows", with a certified metric name and valid provenance attached, and nothing downstream can tell.
It is the same class as the truncation bug `ResultTooLarge` exists for, and it is worse: a wrong
total is wrong for everybody, and this one is wrong for exactly the person who was not allowed to
see it.

**The boot path is a caller of this port already, and it has no subject. So it gets its own identity
type and its own method.**

This is the part earlier versions of this record left silent, and silence here resolves to the
fallback arriving through the back door. The facts, read rather than remembered:
`sutura_app::verify_and_validate` is the only constructor of `Validated`, it calls `verify_anchors`,
`verify_anchors` calls `check_one` for every anchored metric, and `check_one` calls
`warehouse.execute(&plan)`. `LocalService::start` calls `verify_and_validate` during composition,
before a listener is bound and therefore before any caller exists. Every anchor in the bundle is
therefore already executed against the data system by a code path with no subject in scope, and
[pluggable by declaration](0011-pluggable-by-declaration.md)'s TTL refresh will re-run the same path
on a timer, which has no caller either.

Under row-level security this is not a bookkeeping problem. **An anchor's number depends on who asks**,
so an anchor is a number only under a *named* identity - which is exactly the shape this part says has
no signature. Two honest answers were on the table: give impersonating sources a declared verification
identity, or say they have no anchors and let "an unvalidated bundle is never served" mean something
narrower for them. **The first is decided, and it is the only one this design implements.** An
operator who wants the second gets there by authoring no anchors on that source's metrics, which is a
catalog fact rather than a mode: it is not a configuration this record offers, not a fallback, and not
something the absence of a key selects - the absence of a key with an anchor present is a refusal, per
the third bullet below.

```rust
/// The identity the anchor path runs as. Per source, and never derived from a caller.
///
/// A distinct type from `Presented` on purpose: `answer` holds no value of this type and cannot
/// construct one, and `verify_anchor` below accepts nothing else - so the boot credential is not
/// reachable from the request path and a request credential is not reachable from the boot path.
/// The settings layer is its only constructor, from a key an operator wrote in that source's own
/// entry, which is the same provenance the shared acknowledgement in part 5 has.
pub struct VerificationIdentity { /* private */ }
```

and the port gains a second method rather than overloading the first:

```rust
/// Runs one anchor's plan under the deployment's declared verification identity.
///
/// Separate from `execute` so the two identity types cannot be confused at a call site, and so the
/// boot path's result is not a value a handler could return. `AnchorRows` wraps a `RowSet` with a
/// private field and one named accessor.
fn verify_anchor(&self, plan: &QueryPlan, as_: &VerificationIdentity) -> Result<AnchorRows, Self::Error>;
```

**Which identity, per posture, and the asymmetry is the useful part.**

- On a source declared `SharedServiceUser` the verification identity **is** the shared identity, and
  nothing new is configured. That is also the case where an anchor means the most: every caller reads
  that source as that one identity, so the number the anchor certifies is the number every caller
  gets. An anchor is a complete claim there.
- On a source declared `ImpersonationAtSource` the operator declares a verification identity in that
  source's own entry. It is a static, read-only, least-authority credential, it has no default, and
  the settings layer is the only thing that can turn the key into the type - so it cannot be arrived
  at by leaving anything unset, and it cannot be inherited from a neighbouring source.
- **If none is declared, a bundle that declares an anchor on a metric reading that source does not
  boot**, naming the metric and the source. Not skipped, not warned about, and not treated as a
  passing anchor - the three ways this would otherwise become a mode nobody chose. A deployment that
  genuinely wants an impersonating source with no verification identity gets it by authoring no
  anchors on its metrics, which is a catalog fact a reviewer can see rather than a runtime behaviour
  they have to infer.

**Least authority on that credential is a configuration requirement, and the PostgreSQL section above
is why.** A verification identity holding `BYPASSRLS`, or owning the anchored tables where they are
not declared `FORCE ROW LEVEL SECURITY`, certifies the *unfiltered* number - so the anchor passes and
proves less than it appears to. Both exemptions are findings from that section, and this is where they
land as something an operator has to arrange rather than as a note in a survey.

**What this costs `AGENTS.md`'s "an unvalidated bundle is never served", precisely.** The mechanism is
untouched: `Validated` still has one constructor, that constructor still lives in a private module,
and it still executes every anchor against a data system before a bundle can be served. What narrows
is the *reach* of what an executed anchor proves. Before this record the identity was whatever the
process was and nobody had written that down; after it, the identity is declared, and the honest
statement is that **the bundle is proven to compute its certified numbers for one declared identity,
and provenance says which posture each leg actually ran under.** Anything stronger is not available:
under row-level security a per-subject anchor is a function rather than a number, a catalog author
cannot write one, and there is no subject at boot to evaluate it at. Naming the identity is therefore
the whole of the available improvement, and the improvement is real - it converts an unstated process
identity into a declared one whose grants an operator chose.

**The limit on the separation, because it is two types and two methods rather than a proof.** What is
mechanised: `answer` has no `VerificationIdentity` to pass, so routing a question through the boot
credential needs a new parameter on `answer` - a signature change that appears in a diff - and
`AnchorRows` keeps a boot result from being handed back as a result set without a named conversion
somebody wrote. `AnchorRows::expose` is named the way `Secret::expose` is named, to be conspicuous and
greppable, and there is exactly one call site: the anchor comparison. What is **not** mechanised: a
composition root determined to answer a question under the verification identity could write that
conversion, and only review catches it. That is the same class as `Validated`'s own limit, which its
documentation already states - the type proves a warehouse was called, not that it was the right one.

### 2. The subject is the transport's, not the question's

`Query` gains no field, and the reason is not tidiness. A subject on the question is a subject the
**caller** states, and a caller that can state its own identity has none. The existing mechanism
already covers the attempt: `Query` is `deny_unknown_fields`, so a body carrying `subject` is a
parse error naming the field rather than a value quietly ignored.

The subject travels beside the question instead, as a second parameter, all the way down:

```text
transport            validates the token, builds the Caller from its claims
  |                  Caller { subject: Subject, assertion: Secret }
Surface::answer(&self, query: &Query, caller: &Caller)
  |
sutura_app::answer(definitions, query, caller, broker, warehouse)
  |                mints once, for every source the plan reads
CredentialBroker::mint(caller, sources) -> Minted
  |
Warehouse::execute(plan, presented)
```

`Caller` holds two things and neither is optional: who the subject is, and the validated assertion
the broker will exchange. It is built in the transport, from claims, after validation - which is the
`oauth` skill's "build a claims principal once, at the edge, and pass it inward". What the domain
cannot check is that validation happened; a type cannot prove that. What it does buy is that the
absence of a caller is a compile error rather than a default, which is the half that is available.

**"After validation" is a whole subsystem this transport does not have, and it is named here rather
than assumed.** Today `require_token` compares a presented bearer string against a configured one with
`subtle`'s constant-time comparison and builds no principal, and *verified* against the manifests: no
JOSE, JWT or JWKS crate appears in `Cargo.lock` at all, `sutura-http`'s manifest names no
cryptographic dependency, and `ring` resolves only under the default-off `tls` feature's rustls stack
and under `rcgen`, a dev-dependency. So there is nothing to extend. What has to arrive:

- **The mechanism.** A signed JWT bearer token, per
  [RFC 7519](https://www.rfc-editor.org/rfc/rfc7519.html) and the access-token profile in
  [RFC 9068](https://www.rfc-editor.org/rfc/rfc9068.html), verified
  against the issuer's public keys published as a JWKS
  ([RFC 7517](https://www.rfc-editor.org/rfc/rfc7517.html)). The checks are the signature first, then
  `iss` against a configured issuer, `aud` against this deployment's own identifier, and `exp` and
  `nbf` against a clock with a bounded skew. Two of them are the ones that get skipped and each is a
  full bypass: the algorithm has to be checked against a configured allowlist rather than read from
  the token's own header, and the key has to come from the fetched set rather than from anything the
  token points at.
- **The dependencies.** A JOSE or JWT implementation, a cryptographic provider under it, and an
  outbound HTTP client with a key cache for the JWKS endpoint. **All three are new to the graph**, and
  the third is the one that surprises people: it makes the transport an HTTP *client* for the first
  time, so the serving process acquires egress and the request path acquires a dependency on an
  external endpoint being reachable.
- **Which way it fails.** Fail closed, and
  [the plan](0009-the-plan-from-one-source-to-many.md) already states the rule - anything that cannot
  be reached on the authorization path fails closed. A cached key set with a bounded refresh is what
  keeps that from meaning "the identity provider is a single point of failure for every question", and
  it is a cache of *public keys*, which is why it is not the cache `AGENTS.md` bans: there is no
  subject in it to key it on.

**This is the same supply-chain class
[transport security for a source](0010-transport-security-for-a-source.md) flags for a client trust
store, and it was not flagged here until now.** That record's consequence -
"a client trust store enters the dependency graph for the first time. That is a supply-chain change
and belongs in the same review as the source adapter that needs it, not ahead of it" - applies word
for word with "the transport change that needs it" in place of the adapter. It is the larger of the
two: a trust store is data, and this is a signature-verification implementation plus a network client
on the request path. So it belongs in the same review as the transport change that needs it, it does
not arrive with this record, and **nothing in this record is a claim that validation is designed.**
Part 2 designs what the transport hands inward once it has a validated token; obtaining one is its own
piece of work with its own review.

Two consequences worth stating because they are easy to miss. **A deployment with no bearer token
configured currently passes every request through**; under this design that path yields no `Caller`,
and no `Caller` means no credential, which in multi-user mode means a refusal - the correct direction,
and a behaviour change for loopback development, which is what part 5's single-user mode exists to keep
working honestly. And **the token that authenticates the deployment does not become the subject's**: a
shared secret proves the caller holds a configured secret, which is not a claim about which caller.
The two coexist, with the deployment gate where it is and the subject arriving from a validated
per-caller token beside it.

### 3. The port

In the domain, named for what the domain needs. `CredentialBroker` is the name three module comments
and `docs/architecture.md` already use, so it keeps it; what this record adds is a signature.

```rust
/// Mints the credentials one answer needs, all as one subject.
pub trait CredentialBroker {
    /// Why the broker itself failed. Typed per adapter: an unreachable authorization
    /// server and a malformed response are not the same thing to whoever is paged.
    type Error: core::error::Error + 'static;

    /// One call per answer, for every source the plan reads.
    fn mint(&self, caller: &Caller, sources: &SourceSet) -> Result<Minted, Self::Error>;
}

/// A refusal is a result, so minting has the same two-outcome shape `Compiled` has.
pub enum Minted {
    Granted { credentials: LegCredentials },
    Refused { reason: RefusalReason },
}
```

Four things about that signature are load-bearing.

**`mint` takes the whole source set, in one call.** Not one call per leg. This is what makes part 4
possible, and it is also what [RFC 8707](https://www.rfc-editor.org/rfc/rfc8707.html) asks for: the
downstream token has to be audience-restricted to the leg it is for, so N legs need N exchanges with
N different `resource` indicators. One call performing N exchanges is the shape that follows, and it
puts the subject and the deadline in one place instead of N.

**It is synchronous**, because `Warehouse` is and the interior names no framework. A broker adapter
does network I/O and blocks; `sutura-serve` is already synchronous down to one `block_on` for exactly
this reason.

**It asks for impersonation, and it records the delegation itself.** RFC 8693 offers both semantics:
*impersonation*, where the issued token makes the acting party indistinguishable from the subject, and
*delegation*, where the token carries an `act` claim naming both. Delegation is the better fit for
this product, because the data system's own audit would then say "sutura, for this person" rather than
"this person". It is not uniformly available - the exchange endpoint behind the BigQuery path has no
`actor_token` field at all - so the design cannot depend on it. The consequence is a requirement
rather than a shrug: **where the downstream cannot record that sutura acted for a subject, sutura has
to.** The `oauth` skill already states the rule - record the whole principal chain, refusals included,
before returning - and this is why it cannot be satisfied by the data system's log.

**What that requirement is, and what it is emphatically not, because "part of the identity path" reads
as an archive and there is no archive.** Sutura **writes** a record, synchronously, before the outcome
returns, carrying the whole principal chain and covering refusals as well as answers. Sutura **keeps**
nothing: there is no audit store, no retention period and no governance artifact on this side, which is
[the plan](0009-the-plan-from-one-source-to-many.md)'s position and it is the right one - under
impersonation the sources audit each read under the asking subject, and single-user deployments are
development and proof-of-concept shapes. The two halves are compatible because they answer different
questions. Emitting is not delegable, because the `act` claim the downstream would have needed does not
exist on the exchange this design uses; retaining is not ours, because the party that authorized the
read is the party that logs it. And it is a different channel from provenance for a reason worth
stating: provenance rides on the result and a client may drop it, while this record is written whether
the caller reads the answer or not.

**Which makes a port necessary - and it now EXISTS.** When this record was written, `AuditSink`
appeared nowhere and no stack row built one, so it named the shape and stopped. `feat/principal-chain`
then shipped it: the port in `sutura_domain::audit`, and `sutura_runtime::TracingAuditSink` as its first
implementor, because `AGENTS.md`'s rule is that a port arrives with one. The retention half is settled
too, and against this record's own instinct rather than for it - sutura WRITES a record before the
outcome returns and RETAINS nothing, because under impersonation the sources audit under the asking
subject. What it took, for the record, since this paragraph guessed: What it would take: the principal chain, the outcome -
answer or refusal, with the refusal's variant - the sources the plan read and the posture each leg ran
under, and the `not_after` the credentials carried. **State the limit next to the claim:** an emitted
record is worth what the sink behind it is worth, and sutura cannot vouch for a sink it does not
retain. A deployment that attaches a sink which drops records, or none at all, has no audit trail on
this side and the design cannot tell it so - which is precisely why the sources' own logs, written
under the asking subject, are the part that carries the obligation.

**A refusal comes back in the `Ok`.** Minting failures split cleanly: "this subject may not reach
that source" is a governance outcome the caller can be told about, and "the authorization server
returned a 502" is an error. Putting the first in `Err` would let a client library retry a governance
decision until something works, which is the concern `Surface::answer` was already shaped by.

### 4. One asker and one deadline for N legs, and the per-leg posture in the type

This is the part that answers the sentence in `AGENTS.md` with a mechanism instead of an intention -
and the part where an earlier version of this record claimed more than the shape delivered. The
correction is in this section rather than in a footnote, because an overstated control is itself the
defect.

The obvious shape is a credential per leg, each carrying its own subject, and a check that they all
match. That is a check: it can be moved, skipped, or written in one of the two places and not the
other. The shape taken instead **hoists every property that has to agree across legs out of the legs
and into the set, and makes what differs per leg a variant rather than a field somebody reads**:

```rust
/// Everything one answer executes with. One asker, one deadline, N legs.
pub struct LegCredentials {
    /// Who asked. Written once, so N legs cannot disagree about it, because there is
    /// one field. Named `asked_by` and not `subject` deliberately: it is the identity
    /// the question arrived under, and it is NOT a claim that every leg executed as it.
    /// What each leg executed as is the variant in `by_source`.
    asked_by: Subject,
    /// The earliest expiry among everything minted, including the caller's own assertion.
    not_after: Expiry,
    by_source: BTreeMap<SourceName, Presented>,
}

/// What an adapter presents, per leg. THREE shapes, because there are three postures and
/// the third one is not the absence of the other two.
pub enum Presented {
    /// The asker's own bearer credential, minted for this source. `Presented` for a
    /// source declared `ImpersonationAtSource`.
    SubjectToken { material: Secret },
    /// A principal the data system switches to, on a connection the DEPLOYMENT
    /// authenticated, so that the query evaluates as the asker. Not a `Secret`: a role
    /// name is not secret, and the trust here is the connection's rather than the
    /// subject's. Its own variant so that weaker posture is a shape a reader can see,
    /// never a field on the stronger one. Also `ImpersonationAtSource` - the asker's
    /// identity is what the source evaluates against - but by a different mechanism.
    SubjectPrincipal { name: PrincipalName },
    /// **No credential for the asker exists on this leg, and that is what this variant
    /// says out loud.** It carries no material at all: the payload is the operator's
    /// acknowledgement witness from part 5, not a secret and not a placeholder. The leg
    /// executes under the identity the deployment holds for that source, which is not
    /// the asker's, and the answer records that it did.
    SharedServiceUser { declared: SharedIdentityDeclared },
}
```

**The third variant is what makes "no signature without a credential" true rather than hollow.** The
engine that ships is `sutura-exec-datafusion` over local files, and it cannot impersonate anybody: one
process, one operating-system identity. DuckDB is the same. Under a two-variant `Presented` those
adapters would receive a value they ignore, and "the fallback was removed" would mean the fallback came
back as a variant nobody looked at. Under three variants there is nothing to ignore: the shared leg's
value has no credential in it, so an adapter cannot mistake it for one, and a reader of the enum can
see that a third posture exists without reading an adapter.

**And it is what makes the no-downgrade rule enforceable at the adapter rather than promised.** The
broker produces `SharedServiceUser` **only** for a source configured `SharedServiceUser`, and each
adapter matches exhaustively on what it received:

- an adapter for a source configured `ImpersonationAtSource` that receives `SharedServiceUser` returns
  an **`Err`**, not a refusal. It is a wiring defect between the broker and the configuration, nothing
  about the question was wrong, and `AGENTS.md` is explicit that a governance refusal belongs in the
  `Ok` and a hiccup does not - so this surfaces as `SurfaceFailure` and a `503`, and never as something
  a client library can retry into an answer;
- an adapter for a source configured `SharedServiceUser` that receives `SubjectToken` or
  `SubjectPrincipal` returns an `Err` for the same reason and in the same direction. It is the more
  interesting half: an adapter that quietly *accepted* subject material it cannot use would report a
  leg as impersonated that ran shared.

**What one `LegCredentials` value makes true, exactly - two claims, and "every leg runs as the same
subject" is not one of them.**

1. **One asker per answer.** There is one `asked_by` field, `by_source` is private with no `insert`,
   and there is exactly one constructor taking one `Subject`. Two askers in one answer would need two
   `LegCredentials` values, and `answer` takes one - the way `PinnedDefinitions::pin` computes its
   digest from the definitions it stores rather than accepting one as a parameter. Nothing is compared
   to establish this; there is no second place for a disagreement to live.
2. **No leg runs as a third identity, and which of the two it ran as is on the value.** `Presented` has
   three variants and no fourth, two of them mean "the source evaluates this as the asker" and one
   means "the deployment's own identity for this source, acknowledged". So every leg of an answer is
   either the asker or an acknowledged shared identity, per source, and the answer carries which -
   part 5f. There is no representable state in which a leg ran as some other subject.

**What it does not make true, said in the direction that costs us.** A `SharedServiceUser` leg **does
not execute as the asker**, by design and on purpose, so an answer that reads any of those legs is not
an answer every part of which the asker's own permissions filtered. The type makes the *labels* on the
legs agree with what executed; it does not make the effective identities identical, because in a mixed
deployment they are deliberately not. That is why the shared posture needs an operator acknowledgement
at boot (part 5d) and a record in the answer (part 5f) rather than a type alone, and why the governing
sentence about critical data is an operator obligation rather than a check - part 5c says so in full.
`AGENTS.md`'s row reading "every leg runs as the same subject, or the plan is refused rather than
downgraded" is therefore overstated for any deployment with a shared source in it, and narrowing it is
a change to that file.

**Two more limits, both about the broker rather than the shape.** The type does not prove the material
in `SubjectToken` or the name in `SubjectPrincipal` actually authenticates that asker at that data
system: a broker adapter that exchanged the wrong token, or mapped a claim to the wrong role, would
produce a perfectly consistent `LegCredentials` for the wrong person. And it does not prove the broker
consulted the configured posture at all - "produces `SharedServiceUser` only for a source configured
`SharedServiceUser`" is the broker's contract, and what enforces it is the adapter's exhaustive match
above plus *The test that does not exist yet* below. The type closes the failure mode federation
introduces; the adapters close the mismatch; the test closes the one the broker introduces.

The same hoist handles the deadline. `not_after` is one field for the whole answer, the earliest
expiry among the exchanged tokens and the caller's own assertion, so there is one thing to check and
no way for two legs to be checked against different clocks.

**The legs run one after another, and that is decided rather than left to whoever writes the combiner.**
[The plan](0009-the-plan-from-one-source-to-many.md)'s two bounds both key on this answer, so it cannot
stay open: its wall-clock deadline applies to the **sum** of the legs, not to the longest one. Three
reasons, in the order that decided it:

- **`Warehouse::execute` is synchronous and blocking**, and the serving path already moves a question
  onto a blocking-pool thread and `block_on`s the engine from there. Parallel legs mean N of those
  threads per concurrent question, which multiplies a pool size against a concurrency the operator
  configured for one thread per question.
- **A per-subject session is the expensive posture, and parallelism multiplies it.** Part 8's "identity
  fixed at connect" case is a pool per subject; N legs in flight is N per-subject sessions open at once
  per question. Sequential keeps the peak at one session per subject per source, which is the
  difference between a connection budget an operator can size and one that scales with the plan shape.
- **The combine is where a bound already exists**, and running the legs into it one at a time is what
  lets the working-set ceiling refuse before the next leg is even asked for.

**The limit, because the obvious reason to prefer sequential is the wrong one.** Sequential does
**not** halve the memory peak: a hash join needs every leg's build side, so the working-set peak the
plan's ceiling counts is the same either way, and claiming otherwise would be exactly the kind of
overstated control this file's own rules forbid. What sequential buys is threads and sessions, not
bytes.

**And it has a cost parallel does not, which is the honest half.** Under sequential execution a long
first leg can consume the credential's life before the last leg starts, so "the credential aged out
between legs" becomes reachable in a way it is not when every leg starts at once. The check is
therefore **before each leg, against the one `not_after`**, and a leg that would start after it is not
attempted - which is a different case from part 6's expired-token-mid-query, where there is nothing
left to refuse. The domain reads no clock, in either crate, and this design does not give it one: the
check belongs where a clock already is, which is the same place
[the plan](0009-the-plan-from-one-source-to-many.md)'s wall-clock deadline lives.

**Parallel legs are a later change with a measurement behind them, not a tuning knob.** When they
arrive they turn the wall-clock bound from a sum into a maximum and multiply the session peak by the
leg count, so both of that record's bounds move - which is what makes it a decision. `AGENTS.md`'s own
rule for this class is that the numbers deciding a federation shape are wall clock and peak resident
set on a real corpus, not reasoning about allocations.

### 5. Mixed deployments: a posture and an acknowledgement per source, refused before a bundle loads

A deployment is explicitly permitted to hold both `ImpersonationAtSource` and `SharedServiceUser`
sources. That mix is the hardest problem in this record, because the wrong outcome is not a failure -
it is an answer. So it is four typed facts and two checks, and none of them is a sentence anybody has
to remember. The four facts are the declared deployment mode (5a), the per-source posture (5b), the
per-source acknowledgement the shared posture carries (5c), and the posture recorded on the answer (5f);
the two checks are the settings refusal (5d) and the adapter cross-check that
[pluggable by declaration](0011-pluggable-by-declaration.md) owns.

**5a. There are two deployment modes, the deployment DECLARES which it is, and they differ in kind
rather than in degree.**

**Single-user** means credentials are static configuration: one user, one host, not multi-tenant.
There is no per-request identity to establish, so a `SharedServiceUser` source is correct for
**everything** - the one user reads all, by design, and the configured credential is that user's own.
`examples/single-player` is this, and it stays a first-class deployment rather than a degraded one.

**Multi-user** means the caller's identity arrives per request, and a source declared
`ImpersonationAtSource` executes under that caller's own credential. `SharedServiceUser` sources are
still permitted here, and that is the whole difficulty of this part: the deployment has to say so per
source, on purpose.

Stating both is what makes the governing intent precise: **critical data is never served under an
identity that is not the asker's.** In single-user mode that holds trivially, because the configured
credential *is* the one user's - so it does not mean a single-user deployment must acquire per-request
impersonation. In multi-user mode it is an **operator obligation** rather than a check sutura can
perform, for the reason part 5c gives: sutura declares no data sensitivity, so it cannot see which
dataset on a shared source should not have been there. What it can do is make the shared posture
unreachable by accident and unreachable in silence.

```rust
pub enum DeploymentIdentity {
    /// Static credentials, one user, one host. Carries a witness the settings layer
    /// builds from a key an operator wrote, so this mode is unreachable by default.
    SingleUser { declared: SingleUserDeclared },
    /// A subject per request, established by the transport.
    MultiUser,
}
```

The key is **required and has no default**, and the reason is the one `Secret` exists for: getting
this wrong is silent, and the failure is one user's data served to another. A deployment that does not
say which mode it is does not start.

**The mode is DECLARED and is not derived from the source postures, and an earlier version of this
record said both.** The derivation on offer was "every source `SharedServiceUser` is single-user mode,
any source `ImpersonationAtSource` is multi-user mode", and it was justified as one owner per artefact.
It is deleted rather than reconciled, and **not because two owners is untidy - because that particular
derivation is unsound in exactly the configuration that most needs the check.** Work it through: a
genuinely multi-tenant deployment whose sources are *all* `SharedServiceUser` derives to single-user
mode. The acknowledgement in 5d is required in multi-user mode only, precisely because single-user has
one identity to get wrong and it is the right one - so the derivation would exempt from the
acknowledgement the one deployment where every caller reads every source as somebody else's identity.
The failure is silent, it is one user's data served to another, and it arrives by leaving a field out.
A declared mode has none of that: the field exists, it has no default, and no combination of source
postures can answer it on the operator's behalf.

What survives of the "one owner" argument is the summary rather than the fact:
`SecuritySettings::describes_identity()` stops being a `const fn` returning `false` and becomes a
summary derived from the declared mode **and** the per-source postures, printed by the startup log with
the mode on one line and the postures underneath it. A summary derived from two declarations is not a
third declaration.

**5b. The posture is a property of the source, and it is typed.**

Not a `bool`, and not an `Option` whose absence means the permissive thing. Identity gets one closed
enum whose variants each carry the sentence the startup log prints, which is the shape
`TlsTermination` already has in `crates/sutura-config/src/security.rs`:

```rust
/// How a source establishes the identity a query runs as. Printed at startup, per source.
/// **This is the only declaration of this type in this record.** Part 5c adds no second one;
/// it says what the `SharedServiceUser` variant's witness means in multi-user mode.
pub enum SourceIdentity {
    /// The asker's own identity is what the source evaluates against, established per
    /// request - by a minted credential or by a switched principal, which is the
    /// distinction `Presented`'s first two variants carry.
    ImpersonationAtSource,
    /// One identity for every caller. Carries a witness the configuration layer
    /// constructs from a key an operator wrote, so this posture cannot be arrived
    /// at by leaving anything at a default.
    SharedServiceUser { declared: SharedIdentityDeclared },
}
```

**Where it deliberately differs from that precedent, since half-copying it would be the mistake.**
`TlsTermination` derives `Default` and its default is `None`, which is then refused by
`NotFitToServe::TlsTerminationUndeclared` only when the bind is reachable off-host - a default plus a
conditional refusal, and it is right there because a loopback bind genuinely is the case where nothing
needs declaring. Identity has no equivalent condition: there is no bind address that makes "one
identity for every caller" safe to assume. So `SourceIdentity` gets **no `Default` at all**, and a
source that declares no posture is refused unconditionally. `Default` on it would be the guide's own
counterexample - a value that never passed the constructor.

Two things then make it safe rather than merely typed. The `SharedServiceUser` variant carries a
witness only the settings layer can construct, so it is unreachable without an operator writing the
key - and it subsumes the file-backed case the old shape of this part described separately: a CSV
directory is a `SharedServiceUser` source whose operator said so. And the key is **required**, per the
paragraph above. The settings tree has no `sources:` section at all today, so this arrives with one.

There is a second, orthogonal fact: whether the *adapter* can carry a per-subject credential at all.
That is a property of code rather than configuration, so it belongs on the port and not in the file -
and the cross-check is a startup refusal too: `ImpersonationAtSource` configured against an adapter
with no support cannot be deployed. That refusal is
[pluggable by declaration](0011-pluggable-by-declaration.md)'s, stated there and not restated here.
Two facts, one check, at the one moment where refusing is free.

**5c. The requirement is per SOURCE, declared by the operator, because sutura declares no data
sensitivity.**

> **This part was rewritten, not annotated.** The earlier version put an `IdentityRequirement` on the
> catalog's model and derived a metric's requirement upward through its joins.
> [The plan](0009-the-plan-from-one-source-to-many.md) withdrew that: what a person may see lives in
> the data catalog and in their own permissions at the source, and a classification authored here
> would be a second opinion about someone else's authorization. The derived-upward argument is kept
> below because it is the right shape for anything that IS declared per model, and because leaving the
> withdrawal as a banner over a live design produced a record whose next four parts checked a field
> that no longer exists.

So the granularity of the requirement drops from a dataset to a **source**, and what an operator
declares is not how sensitive data is but whether this deployment is willing to serve that source
under one identity for every caller. **This part declares no type of its own**: the shape is 5b's
`SourceIdentity`, and what is decided here is what the witness on its `SharedServiceUser` variant
*means* in multi-user mode. An earlier version re-declared the enum here with different documentation
and the same variants, which is two definitions of one contract - the thing this repository regenerates
rather than duplicates everywhere else.

**In multi-user mode a `SharedServiceUser` source needs a per-source acknowledgement, and there is no
global one.** `SharedIdentityDeclared` - the witness on 5b's variant - is constructed by the settings
layer only from a key an operator wrote **against that source's own entry**, so a deployment cannot
acquire the posture by leaving anything at a default, and cannot acknowledge one source and inherit it
for the next. The key carries the operator's reason as text, printed at startup beside the posture,
because the reason is the part a reviewer needs and the part nobody writes unless the type demands it.
And it is the same witness that part 4's `Presented::SharedServiceUser` carries onto the leg, which is
what ties the boot-time acknowledgement to the thing that actually executed: an acknowledgement no
operator wrote has no value to travel, so there is no leg for it to reach.

**What this loses, said plainly, because it is a real loss of resolution.** Sutura can no longer tell
that a *particular dataset* on a shared source is one the deployment should never have put there. The
check is "this source is served to everyone as one identity, and an operator said so on purpose", not
"critical data is never read as anything but the asker". The second sentence remains the governing
intent and is now an **operator obligation**: critical data goes on a source declared
`ImpersonationAtSource`. Sutura's mechanism makes the shared posture impossible to arrive at
accidentally and impossible to arrive at silently. It does not, and after this withdrawal cannot, look
inside the source and disagree.

**The one shape that would restore the resolution, and it is not built.** A metadata source that
**exposes** a classification - a tag on a dataset in a data catalog - is a label sutura carries rather
than assigns, which is exactly what 0009 permits. Read that way, the pairing check comes back at
dataset granularity, and the direction it must derive in is the argument the withdrawn section got
right: a label belongs to the data, so it lives on the model and a metric's requirement is the
**strictest over the models its plan reads**. Declaring it on the metric would let a metric claim it
needs no impersonation while reading a model that does, and a join is precisely how that happens.
Deriving upward from the models cannot produce that hole. **Nothing here depends on that arriving:**
no metadata adapter exists, and the acknowledgement check above is complete without one.
**5d. The acknowledgement is checked where settings are parsed, and that is the only place its inputs
live.**

Because the posture and its acknowledgement are both per source and both configuration, the check is
**static and complete before any bundle is loaded**:

> In multi-user mode, a source declared `SharedServiceUser` whose entry carries no operator
> acknowledgement is a deployment that **does not boot** - loudly, naming the source and what it would
> have served.

**Where it goes, and an earlier version of this record put it in the wrong place.** It said the check
"extends `verify_and_validate`, already the only constructor of `Validated` and already taking the
warehouse, to take the postures too". That is wrong on its own terms and the next sentence of 5d is
what shows it: `verify_and_validate` takes a bundle and a warehouse and re-runs anchors, while this
check reads the `sources:` tree, the declared mode and the per-source key - configuration only - and
5d itself says a catalog reload does not touch any of them. Putting a configuration check in the bundle
validator means a reload re-runs a check whose inputs it cannot have changed, which is the kind of
mechanism that reads as defence and does the same work twice for nothing.

It belongs in one of the two places that own configuration, and the tree already has both:

- **`Settings::refusals` in `sutura-config`**, which returns every reason this deployment will not be
  served as a `Vec<NotFitToServe>`, is called by `Settings::load` so a caller cannot obtain a
  `Settings` that has not been through it, and is public and side-effect-free so a test can assert on
  the set. It is the exact shape of this check: read the parsed tree, produce typed refusals naming
  what is wrong. `TlsTerminationUndeclared` and `AccessTokenRequired` are the two rows already there
  that this joins.
- **The composition root beside `open_engine`**, for anything that needs a fact `sutura-config` does
  not hold - notably whether the *adapter* for a source can carry a per-subject credential at all,
  which is a property of the linked code rather than of the file.

So the split follows what each half can see: the acknowledgement is a `NotFitToServe` variant, and the
adapter cross-check is a startup refusal in the composition root, which is where
[pluggable by declaration](0011-pluggable-by-declaration.md) already puts it. Neither is in
`verify_and_validate`, and `verify_and_validate`'s own change in this record is a different thing
entirely - the `VerificationIdentity` part 1 gives it, which it needs because it *executes*.

The direction is the one already set in the tree: `open_engine` refuses before the service starts on a
catalog that spans several data systems, on a source this build has no adapter for, and on a missing
data file. A misconfigured deployment therefore never serves a single question, which is strictly
better than one that boots, passes its probes, and refuses a class of questions for as long as nobody
notices.

**And the plan-time refusal is withdrawn with the field it depended on.** The earlier version added
`SubjectOnlyDataOnSharedSource` as defence in depth, on the reasoning that a catalog can be reloaded
while the process lives. That reasoning was sound *while the requirement lived in the catalog*: a
reload could introduce a pairing boot had never seen. It does not survive the requirement moving to
the settings tree, which a reload does not touch - so there is no pairing a reloaded bundle can
introduce, and a variant no test can provoke is one `AGENTS.md` says the enum refuses to carry.
**Deleted rather than kept as a decorative arm.** What a reload can still do is name a source that is
not configured, which the existing source-resolution refusal already covers.

**5e. Flipping a deployment from single-user to multi-user re-evaluates every source.**

Stated as a rule rather than left implicit, because it is the transition that turns a correct
configuration into a dangerous one **unchanged**. A single-user deployment legitimately holds every
source under one static credential - that credential *is* the one user's. The same file in multi-user
mode serves every one of those sources to every caller as one identity. So: **the declared mode is an
input to the settings check, and changing it re-runs every source's** acknowledgement rather than any
incremental view of what changed. The refusal is most of what makes the transition safe - a deployment
that flips the mode and has acknowledged nothing does not boot - and the rest is that neither the
posture nor the acknowledgement has a default, so there is no source the check cannot see.

**This is also the transition the declared mode of 5a exists for.** Under the derivation 5a deletes,
this section would have had nothing to describe: there would be no mode field to flip, and a deployment
would change its posture by editing a source entry, which is exactly the incremental edit the check is
arranged not to trust. One field, no default, and flipping it re-evaluates everything.

What this gives `AGENTS.md`'s untested sentence is teeth, and part 4 says how much. "Every leg runs as
the same subject, or the plan is refused rather than downgraded" is **not** what the shape delivers, and
what it delivers instead is three checkable parts: one asker per answer, because `LegCredentials` has
one `asked_by` field (part 4); no leg under a third identity, because `Presented` has three variants and
no fourth (part 4); and a leg under a shared identity only where an operator acknowledged that source,
refused at settings-parse time otherwise (5d), with the posture recorded on the answer (5f) so a reader
can tell which kind of answer they are holding.
**5f. Provenance records the mode per leg, beside the definition digest and not under it.**

An answer must not be mistakable for impersonated when it was not, so `Provenance` grows a per-source
execution record: which posture each leg ran under, and who asked.

**It is read off what executed, not off configuration, and that is the whole of the mechanism.** The
per-leg posture comes from one exhaustive match over the `Presented` value the adapter was actually
handed - `Presented::executed_as`, one function so there is one answer - and not from the
`SourceIdentity` in the settings tree. The two are meant to agree, and the adapter's own exhaustive
match in part 4 is what refuses them when they do not; but if they ever disagreed, the record has to say
what *ran*. A provenance field derived from configuration would report a leg as impersonated on the
strength of a file, which is the one thing this field exists to stop. So the leg that received
`Presented::SharedServiceUser` is recorded as shared, whatever the file says, and there is no arm that
produces "impersonated" from anything other than a subject variant.

**Beside the digest, not under it, and the reason is the digest's own purpose.** The definition digest
is over the definitions, so that two deployments serving the same catalog certify the same numbers.
Identity posture is deployment configuration rather than authored content - a different owner - and
hashing it in would make the same catalog produce two digests in two deployments, which is the one
property the digest exists to have. That is also why it differs from the knowledge declaration, which
*is* under the digest: knowledge is content the catalog author wrote, and it changes what the prompt
claims.

**5g. The inference risk, stated without overclaiming.**

A plan may join a leg read as the asker to a leg read under a shared identity - which, after part 4,
is not a hypothetical arrangement but the two variants of `Presented` appearing in one
`LegCredentials`. **Defended:** the impersonated leg's rows are filtered by its own data system, so the
joined output is constrained by what the asker may see on that side, and the shared leg contributes
only data from a source an operator acknowledged as served under one identity for everybody.

**Not defended, in two parts.** The acknowledgement is a trusted precondition and a coarse one - it
says an operator meant to share that source, not that everything on it is shareable, and after part
5c sutura has no dataset-level view to check it against. That is the same class as a relationship's
declared cardinality, which nothing checks against the data either, and it is the price of not holding
a second opinion about someone else's authorization. And the concrete inference
channel is one the sibling record found independently:
[federating across different data systems](0007-federating-across-different-data-systems.md) declines
a semi-join because pushing one source's key values into another's statement puts that source's data
into the other's query log and plan cache. Critical values arriving in a shared source's log is
exactly that channel, and the defence is architectural rather than a check - results are combined
after the fact rather than one leg's keys pushed into another's statement. If that decision is ever
revisited, this is why it cannot be revisited on performance grounds alone.

### 6. What is refused, and what is not a refusal at all

A refusal is a result. Following ADR 0005, each new variant needs a status, and adding one to the
domain fails to compile in `sutura_http::wire::refusal` until somebody assigns it.

**This record adds exactly one refusal variant.** It proposed two, and the second is deleted below
rather than kept as an arm nothing reaches.

| Outcome | Variant | Status | Why |
| --- | --- | --- | --- |
| The subject has no credential at that source | `CredentialUnavailable { source }` | `403` | Understood, and refused. Asking differently does not help; a different subject or a grant does |
| The broker could not be reached, or answered something unparseable | not a refusal | `503` | `SurfaceFailure`, from the `Err` side. Nothing about the question was wrong |
| The broker produced a posture the adapter is not configured for | not a refusal | `503` | A wiring defect between the broker and the source declaration, per part 4. Nothing about the question was wrong, and offering it as a refusal would invite a retry loop over a deployment bug |
| The caller's own assertion is expired or invalid | not a refusal | `401` | The transport rejects it before the domain sees a `Caller` at all |

**This is the amendment to ADR 0005**, which states that "the `403`s are not a statement about a
credential" because at the time no token widened anything. `CredentialUnavailable` is one, so that
sentence stops being true the day this lands - and its `detail` must not send a caller looking for a
better deployment token, because what is missing is a grant at the data system.

**`SourceCannotImpersonate` is DELETED, and this is the record correcting its own invented debt.** An
earlier version of this table carried it at `409`, for "the source is arranged so that nobody can be
impersonated on it". Walk the configurations that were supposed to reach it, after 5c moved the
requirement from the catalog to the settings tree, and there is nothing left:

- **A source declared `ImpersonationAtSource` on an adapter that cannot impersonate** is refused at
  boot, by [pluggable by declaration](0011-pluggable-by-declaration.md)'s startup check. It never
  serves a question, so no question can be refused by it.
- **A source declared `SharedServiceUser` in multi-user mode** answers, under the acknowledged shared
  identity, and records the posture on the answer. That is the decided behaviour of 5c and 5f - the
  whole point of the acknowledgement is that this case is permitted rather than refused - so refusing it
  at request time would contradict the section above it.
- **A source declared `SharedServiceUser` in multi-user mode with no acknowledgement** does not boot
  (5d). Again no question reaches it.
- **A subject with no credential at a source that does impersonate** is `CredentialUnavailable`, which
  is the row that stays.
- **A broker handing an adapter the wrong posture** is an `Err`, per part 4 and the row above: it is a
  deployment defect and not a governance outcome.

So there is no configuration that reaches a plan-time `SourceCannotImpersonate`, and `AGENTS.md` is
explicit about what to do with that: a `RefusalReason` variant no test can provoke is one the enum
refuses to carry. It is deleted rather than left as defence in depth, for the same reason 5d deletes
`SubjectOnlyDataOnSharedSource` - an arm that reads as a mechanism and resolves to nothing spends a
reviewer's trust on nothing. The credential-port step in
[the plan](../implementation-plan-identity-and-services.md) listed it when this was written and no
longer does; the variant exists in no record and in no branch.

**An expired token mid-query is deliberately not on that list**, and the reason is worth more than a
variant. Once a leg is running there is nothing for sutura to refuse: the data system decides what
happens to a session or a job whose credential ages out, and we cannot un-authorise work already in
flight. What is available is a floor, checked before the first leg starts, on how much life the
credential has left relative to what the query may take - and that check belongs in the broker
adapter, because it is the only component that has both a clock and the configured query timeout. The
domain reads no clock today, in either crate, and this design does not give it one: `Expiry` is
carried so an audit record can name it, exactly as `TimeRange` carries dates the caller resolved.

**Part 4's sequential legs add a second place that floor is checked, and it is not this table.** A leg
that would start after `not_after` is not attempted, which is a case the parallel shape does not have.
It is the same check against the same one field, before each leg rather than only before the first, and
it lives in the same component for the same reason: the domain reads no clock.

Which means `Expiry` is, for now, a field the domain holds and nothing in the domain reads - the shape
`ParamValue::Integer` had before it was deleted, stated rather than hidden. It arrives with the broker
adapter that refuses on it or the audit sink that prints it, and a variant for "expiring too soon"
arrives the same way: a fake broker can provoke `CredentialUnavailable` from the day the port exists,
and could not provoke that one.

### 7. `Secret` composes; it does not grow an expiry

`Secret` is unchanged: one property - opacity - an infallible constructor because every string is a
valid secret, a hand-written redacting `Debug` and `Display`, and no `PartialEq`, so `==` on
credential material does not compile.

An expiring token is not a secret with a date on it. Adding `not_after` to `Secret` would put an
invariant on a type whose constructor correctly has none, and would make the field either optional - a
pre-shared bearer token does not expire - or a lie for the values that have none. So the expiry lives
one level up, on `LegCredentials`, hoisted where it belongs anyway, and
`Presented::SubjectToken` holds a plain `Secret`.

**And two of `Presented`'s three variants hold no `Secret` at all**, which is the type saying something
true rather than an omission. `SubjectPrincipal` carries a role name, and a role name is not secret -
the trust there is the connection's. `SharedServiceUser` carries the operator's acknowledgement witness,
and there is no credential for the asker on that leg to redact. A single `Secret` field on one enum
would have made all three look like credential material, which is the opposite of what part 5f has to
be able to read.

`Subject` needs care and is not a `Secret` either. A subject identifier has to reach an audit record,
so redacting it entirely defeats what it exists for, while printing it freely puts a caller identifier
into every log line that formats a struct containing it. So `Subject` derives no `Debug`: it gets a
hand-written one printing a truncated digest - `sha2` is already one of the domain's four dependencies
- plus a named, greppable accessor for the sink allowed to record the real value, the way
`Secret::expose` is named to be conspicuous.

**The limit, again with the claim.** A digest of a low-entropy identifier is not anonymisation - an
address or a username is a dictionary attack away. It buys correlation between log lines without the
identifier travelling to every collector that reads them, which is a smaller claim and the true one.

### 8. The pooling discipline belongs to the adapter, and each one declares which posture it has

The worst outcome this system can produce is a question answered as the wrong subject, and the
cheapest way to produce it is a pooled connection returned dirty. So the discipline is part of the
decision rather than an implementation note, and it is written per adapter because the three systems
do not offer the same thing.

Three postures. An adapter has exactly one, and says which:

**No connection identity.** The credential travels on each request and there is nothing to clean;
BigQuery is this. The hazard moves to caching, and the rule follows the one `AGENTS.md` already states
for result caches: **keyed by subject first, or it does not exist.** That now covers credential caches
too, for the same reason - a cache keyed by anything else is a cross-subject read under row-level
security.

**Identity fixed at connect.** A pool per subject, connections never shared. Correct and it does not
scale, so it is the posture for a source whose sessions cannot be re-identified, and a reason to
prefer a source whose can. This is the posture part 4's sequential legs were chosen with: N legs in
flight would be N per-subject sessions open at once per question, and one at a time keeps the peak
where an operator can size it. It is also the posture PostgreSQL option B lands in, which is what the
rewritten fourth assertion of *The test that does not exist yet* tests.

**Identity switched on a borrowed connection.** A shared pool, a switch on borrow and a reversal on
return. The posture that scales and the one that fails silently, so it carries three rules:

1. **The reversal is in the pool's check-in path, never in the query path.** A cleanup written after
   the query is a cleanup that an early return skips, and in a codebase where `?` is the normal way
   to propagate an error, "after the query" means "on the success path only".
2. **Check-in verifies rather than assumes.** Ask the connection who it is; if the answer is not the
   pool's own identity, **close the connection instead of returning it.** That converts a silent
   wrong-subject answer into a lost connection, which is the correct exchange rate, and it is the
   fail-closed direction `AGENTS.md` asks for on the query path.
3. **Nothing the catalog wrote may execute inside the switched session without passing the shape
   guards.** This is where the record touches
   [the authored-SQL escape hatch](0004-a-named-escape-hatch-for-authored-sql.md), and the connection
   is not obvious: on a system where the switch is reversible from SQL, a fragment that reset the
   identity would read the pool's rows under the subject's question. Two existing guards cover it, and
   both were checked rather than assumed - a statement is not an expression, so
   `Construct::SchemaStatement` refuses one and the keyword does not parse in expression position
   anyway; and called function names are an **allowlist** of thirty-two aggregate and scalar names in
   `sutura_sql::expression::vocabulary`, so a session-setting function is refused by absence. That
   allowlist fails closed, which is the property that matters: the day a dialect grows a new way to
   change the current role from inside an expression, the guard needs no update.

## Consequences

### What one process and one connection forecloses

The alternative route to a second data system is DuckDB `ATTACH`: one process, one DuckDB connection,
one attachment per remote database. It is the cheaper route to *federation* and it is the route that
forecloses this record.
[Federating across different data systems](0007-federating-across-different-data-systems.md) makes the
architectural case and measures the extensions, so this section adds only the identity half and does
not restate it.

The identity half is two facts. `DuckDbWarehouse` holds one `duckdb::Connection` for its whole life
and `Warehouse::execute` takes `&self`, so an identity that is a property of the adapter cannot be a
property of the request - the same reason part 1 changes the signature rather than the adapter. And
*verified:* an attachment carries the credentials given at `ATTACH` time for its whole life, with the
PostgreSQL extension opening its **own pool** behind one attachment, sixty-four connections by
default. One attachment is one remote identity spread across many connections, the exact inverse of
what this record needs. Per-subject identity would then mean an attachment per subject - N remote
namespaces in one catalog, where the isolation between them is a name our own renderer is trusted to
get right rather than anything the data system enforces - or `DETACH` and `ATTACH` per request,
serialising the whole process behind one connection.

None of that makes `ATTACH` a bad tool. It makes it a tool for a deployment whose answer to "as whom"
is "as the service" - which, in this record's vocabulary, is single-user mode, where it is a perfectly
good answer. The decision for multi-user mode is therefore: **per-source adapters, each able to
establish a session per subject.**

### What that costs, plainly

- An adapter, a driver and a dialect per data system, rather than one extension per data system.
- A pool per source with a per-subject identity discipline - the part that is easy to get wrong, and
  the reason *The test that does not exist yet* has a dirty-connection case.
- Oracle is a driver question before it is an identity question. *Verified:* the long-standing Rust
  path, the `oracle` crate over ODPI-C, dynamically loads a proprietary client library at runtime - a
  poor fit for the distroless artifacts and for musl. *Verified:* Oracle now also maintains a pure-Rust
  thin-mode driver needing none, in beta as of this writing, so that objection is weakening - but
  **its feature list does not mention proxy or token authentication**, the one capability this design
  needs from it, so that has to be confirmed against the driver rather than against the database.
- `LocalService<W>` becomes `LocalService<W, B>`. It erases both, so a handler stays concrete.
- **`LocalService::start` takes the verification identities, and `LocalService` keeps them.** It has to:
  `verify_and_validate` needs one per source to run the anchors at all, and
  [pluggable by declaration](0011-pluggable-by-declaration.md)'s TTL refresh re-runs that path with no
  caller in scope, so the value has to outlive composition. That is the one static credential a
  multi-user deployment holds, and part 1 says what it must not hold at the data system.
- **The `Warehouse` port grows a second method, and every adapter implements it.** Two adapters today,
  and it is a real cost rather than a free one: a shared method taking an identity enum would have been
  one signature instead of two. Part 1 says why it is the wrong trade - a single method cannot give the
  boot path a different return type, and the return type is what stops a boot result being served as an
  answer.
- **`sutura-config` grows the whole `sources:` tree**, and with it the per-source posture, the
  acknowledgement key and its operator reason, and the verification identity. `Settings::refusals`
  grows the `NotFitToServe` variant 5d names. The settings tree has no `sources:` key at all today, so
  none of this is an edit to an existing shape.

## The test that does not exist yet

`AGENTS.md` says this test cannot be written until a credential exists per leg. Naming it precisely
is what makes the invariant real later, so here it is named, including the parts that stop it from
being a test that passes for the wrong reason.

**PostgreSQL first, not BigQuery.** The fixture has to be one CI can stand up with no cloud account
and no tenant, and the enforcement has to be the data system's rather than ours.

**The mechanism it is written for is `oauth`, not `SET ROLE`, and an earlier version of this section had
it the other way round.** That version tested two roles a pooled service switched into, with a fourth
assertion on `SET ROLE NONE` versus `RESET ALL`. [The plan](0009-the-plan-from-one-source-to-many.md)'s
Decision 1 and `docs/implementation-plan.md`'s Postgres-over-OAuth step both **reject** that mechanism -
per-subject connections authenticated as the subject, not role switching on a shared pool - so a test
written for it describes a fixture nobody is building. It is rewritten below for what the plan builds.
The `SET ROLE` findings earlier in this record keep their place, as the evidence for why the mechanism
was rejected rather than as a suite: if a deployment ever chooses option A anyway, the `SET ROLE NONE`
assertion is the one it needs, and this paragraph is where to find that it was written down.

**The fixture.** One table with a column the policy discriminates on, row-level security enabled
**and forced**, so the table's owner does not bypass its own policy - a fixture where the owner is
exempt is a fixture that proves nothing. Two roles, neither of them the owner, neither holding the
attribute that bypasses row-level security, both granted `SELECT`. A policy that reads the identity
in effect rather than a session variable the client sets, because a variable the client sets is our
own claim rather than the data system's check.

**The two identities.** Two subjects whose tokens the validator accepts and whose claims the server
maps to those two roles - by the validator itself or by an identity map, which is the mechanism's own
mapping step and not ours. Not one subject asked twice, and not two subjects mapped to the same role:
the assertion is about two identities seeing two things. Under this mechanism `session_user` **is** the
subject and is immutable for the connection's life, which is what several of the assertions below read.

**What it asserts**, in the order that matters:

1. The same certified question - same metric, same grain, same range, no dimensions - asked as each
   subject returns **different** rows, and each equals what that role is permitted to see.
2. A third run under the deployment's own identity returns **more** rows than either subject. This is
   the assertion that catches a fixture whose policy is not actually on, which is the way this test
   fails to a false green.
3. The negative control: the same fixture with row-level security disabled gives both subjects the
   **same** rows. A test that cannot distinguish the two states is not measuring the two states, and
   this is what makes the first assertion red before the mechanism exists and green after.
4. **The connection is never reused across subjects, and never reused past `not_after`.** This is the
   wrong-subject case rewritten for the mechanism the plan builds, and it is two assertions because
   `oauth` removes one hazard and leaves the other standing.
    - **Across subjects.** A connection borrowed to answer for subject B reports **B** in
      `system_user`, asked in-session. Under `oauth` the connection is authenticated as the subject and
      a non-superuser's `session_user` is immutable for its life, so the only way to get A's identity
      on B's question is a pool that keyed a connection by something other than the subject. Asserted
      by asking the connection rather than by inspecting our own pool's bookkeeping, because our
      bookkeeping is the thing under test. There is nothing to assert about `SET ROLE NONE` or
      `RESET ALL` here: no role is switched, so there is no reversal to get wrong.
    - **Past the deadline.** *Verified above and this is what makes the assertion necessary:* nothing
      re-checks a token mid-session, so **a session outlives the token that opened it.** A connection
      opened for a subject is therefore not handed to a second question for that subject once the
      `not_after` on part 4's `LegCredentials` has passed - it is closed and replaced. The assertion is
      that a question asked after the deadline opens a **new** connection under a **freshly minted**
      credential, and never reuses the old one, which is measurable by the connection's backend
      identifier changing. Without this assertion the pool is the one component that quietly converts a
      short-lived credential into a long-lived authorization.
5. A subject with no mapping is refused as `CredentialUnavailable`, and the assertion is on the
   refusal **and** on the absence of rows. A refusal that arrived after a successful read under
   somebody else's identity is not a refusal.
6. The **view** case, because it is the one that voids the mechanism while every other assertion
   still passes: the same metric resolved through a view whose base table has row security, with the
   view declared `security_invoker = true`, returns the subject's rows; the same view without it
   returns the view owner's. The second half is an assertion that the leak exists, which is what
   makes it worth having - if sutura later refuses such a bundle, this is the case that proves the
   refusal is aimed at something real.

PostgreSQL also offers a **fail-loud** setting worth using in the fixture rather than only in
assertions: with `row_security` turned off, a query that *would* have been filtered by a policy raises
an error instead of returning rows. A fixture that sets it and expects the error is a direct check
that the policy is engaged, independent of counting rows.

**And four tests for the mixed deployment, none of which needs a real data system.** These are the
ones that arrive with the port rather than with an adapter, and they are **unit tests of
`Settings::refusals`** rather than tests of the bundle validator, which is 5d's correction turned into
where the file lives. The two `NotFitToServe` rows already there are the precedent for the shape.

1. Multi-user mode, a source declared `SharedServiceUser` with no operator acknowledgement on its
   entry: `Settings::refusals` returns the variant, naming the source, and `Settings::load` therefore
   refuses. Asserted **on the typed variant and not on the message**, the way every existing refusal
   test in that module is.
2. The same deployment with the acknowledgement present produces **no refusal**, and the startup log
   carries the posture and the operator's stated reason per source. Without this half the first test
   passes against a build that simply refuses every shared source, which is a different mechanism.
3. **Single-user mode with every source `SharedServiceUser` produces no refusal, with no
   acknowledgement key anywhere.** This is the test that stops the mechanism from being a ban: the
   permitted deployment has to keep working, or the next person deletes the check. It is also where the
   mode matters - the acknowledgement is required in multi-user mode only, because single-user has one
   identity to get wrong and it is the right one.
4. **The transition test:** that same single-user configuration, with only the declared mode flipped to
   multi-user, **refuses.** One field changes, nothing else, and the outcome inverts. That is part 5e as
   an executable assertion rather than a rule, it is the one that would catch a future incremental check
   that looked at only what changed, and it is only writable because 5a made the mode a declared field:
   under the derivation 5a deletes there is no single field to flip.

**And four tests for the identity types and the boot path, also against fakes.** These are what stop
part 1 and part 4 from being prose:

1. `an_execution_without_a_credential_does_not_compile` - a `compile_fail` doctest calling
   `execute(&plan)` with no second argument, with a **compiling twin** that passes a `Presented`, so a
   rename cannot make it pass vacuously. This is the pair `Validated` already has two of.
2. `a_second_leg_cannot_carry_a_second_asker` - a `compile_fail` doctest, and it tests the property that
   is actually true rather than the one an earlier version of this record claimed. There is no
   constructor taking a per-leg subject, and `by_source` is private with no `insert`, so a leg cannot be
   added or replaced after minting. **Its compiling twin is a broker adapter in another crate
   constructing a `LegCredentials` and returning `Minted::Granted`** - which is the half that pins the
   constructor as `pub`, so a future change tightening it to private breaks the twin and gets noticed
   rather than silently removing the broker's only way to return a value.
3. `a_shared_leg_is_recorded_as_shared_whatever_the_configuration_says` - a fake warehouse handed
   `Presented::SharedServiceUser` while its source entry says `ImpersonationAtSource`, asserting the
   provenance record reads shared. It pins 5f's "read off what executed" against the easier
   implementation that reads the settings tree.
4. `answer_cannot_pass_a_verification_identity` - a `compile_fail` doctest, with a compiling twin that
   passes one to `verify_anchor`. It pins part 1's separation at the one place it is mechanised.

**Plus two on the anchor identity, which need a warehouse fake and no data system.** A bundle declaring
an anchor on a metric whose source is `ImpersonationAtSource` with **no** verification identity
configured **does not start**, naming the metric and the source - and the same bundle with one
configured starts and validates. The second half is what stops the first from passing against a build
that refuses every anchor.

**What a fake can and cannot do.** The fake broker that ships with the port can provoke
`CredentialUnavailable`, the adapter-mismatch `Err`, and **all three** `Presented` shapes, which is what
makes the refusal corpus complete without a data system - that is what fakes are for here. It cannot
assert the six numbered assertions above: a fake warehouse answering the same rows for every subject
would be a test that looks like coverage and measures nothing, which `AGENTS.md` says is worse than no
test. So the split is deliberate: **the refusal corpus and the configuration refusals are fakes and
unit tests, and the two-subject assertion is a real data system in a container, registered in the
`tests/adapters` matrix the way every other data system is.**

## Alternatives considered

**A service identity plus a filter we apply ourselves.** Read as the service, then remove the rows the
subject may not see. Rejected: it moves an access decision out of the system that owns the data into a
runtime that would have to reimplement the policy, correctly, for every source, forever. The first
time the two disagree the answer is wrong and certified, and every source's grants become decorative.

**A pool per subject as the general answer.** Rejected for that; kept as the honest answer for a
source whose sessions cannot be re-identified, which is why part 8 writes the discipline per adapter.

**A subject on the `Query`.** Rejected in the strongest available terms: a caller that states its own
identity does not have one. `deny_unknown_fields` already makes the attempt a named parse error.

**A credential per leg with a check that the subjects match.** Rejected for a shape where the question
cannot be asked - part 4. A check can be skipped; a single field cannot.

**A two-variant `Presented`, with a shared source receiving whichever variant fits worst.** Rejected,
and it is the alternative this record shipped for one revision. Under it the file engine - the one that
actually ships - receives a value it ignores, so "there is no code path that executes without something
a broker produced" is satisfied by a value that means nothing, and part 5f has nothing to read to tell
a shared leg from an impersonated one. The third variant costs one arm in every adapter's match and buys
the only thing that makes the no-downgrade claim checkable.

**One `execute` taking an identity enum, `RunAs::{Subject, Verification}`, instead of a second port
method.** Rejected, and it is close. It is equally strong on the axis that matters most - `answer` holds
no `VerificationIdentity` and so cannot construct the second arm - and it costs adapters one method
instead of two. What it cannot do is give the boot path a different **return type**, and that is the
half that keeps a boot result from being handed back as a result set: under one method, rows are rows.
Two methods plus `AnchorRows` is the shape that makes the misuse a named conversion in a diff rather
than an ordinary return.

**Deriving the deployment mode from the source postures.** Rejected as unsound rather than as untidy,
and 5a works the case through: a multi-tenant deployment whose sources are all `SharedServiceUser`
derives to single-user mode, and single-user mode is exactly where the acknowledgement is not required.
The derivation would therefore exempt the deployment that needs the check most, by leaving a field out.

**A defaulted `dry_run` returning `Ok(())` once it takes a subject.** Rejected: with a subject in the
signature, `Ok(())` from an adapter that did not look is indistinguishable from `Ok(())` from an adapter
that asked and was told yes, so the default would answer "this subject may run this" on behalf of every
adapter that declined to implement one. `PreFlight::NotAsked` keeps the default and removes the claim.

**Parallel legs.** Not rejected - deferred, with the measurement named. Part 4 decides sequential for
threads and per-subject sessions rather than for memory, states that the working-set peak is unchanged
either way, and says what moves in [the plan](0009-the-plan-from-one-source-to-many.md)'s two bounds
when parallel arrives.

**One `mint` call per leg.** Rejected because it puts the subject and the deadline in N places, and
RFC 8707 wants N audience-restricted tokens from one decision anyway.

**Waiting for federation to decide any of this.** Rejected. Part 1's signature change is the whole
mechanism, and it is one line on a port today against every adapter and composition root later.

## What has to be true before any of this becomes an invariant

Nothing in this record may be cited as an invariant, and `AGENTS.md`'s Invariants table gets no new
row from it. What would earn one, in order - and **each row says only what the shape enforces**, which
is why two of them are worded more narrowly than the sections above were in an earlier revision:

| Row it could become | What has to exist first |
| --- | --- |
| A leg cannot execute without a credential | The `Warehouse::execute` signature above, and every adapter compiled against it. **And the second method:** `verify_anchor` is the only other way into a data system, it takes a `VerificationIdentity` and not a `Presented`, and `answer` holds no value of that type - so the boot path is not a hole in this row rather than being outside it |
| One answer has one asker | `LegCredentials` with one `asked_by` field, one constructor taking one `Subject`, a private `by_source` with no `insert`, and the `compile_fail` doctest plus its out-of-crate compiling twin. **NOT** "every leg runs as the same subject": a `SharedServiceUser` leg deliberately does not run as the asker, and part 4 says so at length |
| No leg of an answer runs as a third identity | `Presented`'s three variants and no fourth, plus each adapter's exhaustive match refusing a posture it is not configured for as an `Err`. What this bounds is the SET of identities a leg may run as - the asker, or that source's acknowledged shared identity. It says nothing about whether the material in a subject variant authenticates the asker; that is the broker's contract and the two-subject test's job |
| A source that reaches its data under one identity for everybody says so at startup | The `sources:` configuration section, the required and defaultless `SourceIdentity` per source, the required and defaultless `DeploymentIdentity`, and `describes_identity()` deriving a summary from both |
| A source served under one identity for everybody was acknowledged on purpose | The `sources:` section, `SharedIdentityDeclared` constructible only from a per-source operator key, the multi-user refusal as a `NotFitToServe` variant returned by `Settings::refusals`, and the four settings tests above. Note what this row does NOT say: nothing here reaches dataset granularity, and the stronger sentence stays an operator obligation until a metadata source EXPOSES a classification to carry |
| An answer cannot be mistaken for impersonated | `Provenance` carrying the per-leg posture, beside the digest, and a golden that shows it. **The posture has to come from `Presented::executed_as` over the value the adapter received**, not from the settings tree - a field derived from configuration would report what was configured rather than what ran |
| A subject with no credential at a source is refused, not answered | `CredentialUnavailable`, its status in `sutura_http::wire::refusal`, and a fake broker that provokes it. **One variant, not two:** `SourceCannotImpersonate` is deleted, because after 5c and the boot refusals no configuration reaches it |
| An anchor is verified under a declared identity, or the bundle does not boot | `VerificationIdentity` per source with the settings layer as its only constructor, `verify_anchor` on the port, and the two anchor tests above. **What this row would NOT say:** that the certified number is the number a caller sees. Under row-level security it is the number the verification identity sees, and part 1 explains why nothing stronger is available |
| Two subjects get different rows | The whole test above, against a real data system, in the adapter matrix, over the `oauth` mechanism the plan builds |

Until then this is a design, and `AGENTS.md`'s *Built And Not Wired* section is the precedent for
what to do with a claim whose mechanism does not exist yet: write it where a reader cannot mistake it
for the table.

**Two things in `AGENTS.md` this record now contradicts, and they are changes to that file rather than
to this one.** Recorded here so the contradiction is visible from the record that caused it. First, the
*Changing The Query Path Or The Tool Surface* row for a second execution leg reads "every leg runs as
the same subject, or the plan is refused rather than downgraded"; part 4 shows that is not what the
shape delivers in a deployment with a shared source, and the accurate version is the pair of claims in
the table above. Second, the *Invariants* row for "an unvalidated bundle is never served" keeps its
mechanism untouched and needs its reach stated: an anchor is verified under a declared verification
identity, which is not necessarily any caller's.
