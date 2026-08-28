---
title: A credential per leg, for the calling subject
description: What end-to-end impersonation concretely requires of BigQuery, PostgreSQL and Oracle, why the credential is minted per request and never falls back to a service identity, the signature of the CredentialBroker port, why one subject and one deadline are hoisted above N legs so two legs cannot disagree, how a deployment may mix impersonating and non-impersonating sources without serving critical data as the wrong principal, what is refused rather than degraded, and the two-subject test that makes the invariant real.
---

# A credential per leg, for the calling subject

Status: **accepted as a design, and nothing in it is built.** No line of this record describes code
in the workspace. It decides the shape of the identity path before the first adapter that needs one,
because both halves of that path are cheap to decide now and expensive to retrofit: the transport has
to learn a subject, and the execution port has to stop being able to run without one.

**Superseded in one part, by the plan.** Where this record has sutura declaring a dataset critical -
per model, derived upward through joins - that is withdrawn. Sensitivity lives in the data catalog and
in the asking person's own permissions at the source, which is what impersonation exists to reach, and
a classification here would be a second opinion about someone else's authorization. What survives is
everything about the credential, the chain, the mode a source declares, and the refusal for a mode the
deployment cannot perform. See
[the plan](0009-the-plan-from-one-source-to-many.md) and
[pluggable by declaration](0011-pluggable-by-declaration.md).

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
| No subject reaches execution | `crates/sutura-domain/src/warehouse.rs` | `Warehouse::execute(&self, plan: &QueryPlan)`. There is no second parameter, and `&self` means an adapter's identity is a property of the adapter rather than of the call |
| No subject reaches the application | `crates/sutura-app/src/surface.rs` | `Surface::answer(&self, query: &Query)` |
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

`CVE-2016-2193` was a cached plan generated for one role reused under another, applying the wrong
policy set, and the upstream commit names the triggering pattern as a common user and query planned
once and reused across multiple `SET ROLE`s. `CVE-2023-2455` was policies disregarding user ID changes
after inlining; `CVE-2024-10976` the same below subqueries. And `CVE-2026-14666`, published
2026-08-13 and fixed in 18.5 and the other supported branches, is row security caching disregarding
role modifications, where stale policies continue "until some other event invalidates the cache or
connection termination ends the session".

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
mechanism, and the last four are what has to be declared, refused, carried and cleaned up.

### 1. A subject on the execution leg, and never a fallback

**A credential is minted per request, for the calling subject, for each leg of the plan. A leg that
cannot run as the subject is refused. There is no service-identity fallback, and its absence is
structural rather than a rule somebody follows.**

`Warehouse::execute` gains a parameter and loses the ability to be called without one:

```rust
fn dry_run(&self, plan: &QueryPlan, as_: &Presented) -> Result<(), Self::Error> { Ok(()) }
fn execute(&self, plan: &QueryPlan, as_: &Presented) -> Result<RowSet, Self::Error>;
```

`dry_run` takes it too, and not for symmetry: a pre-flight asks "would this be accepted", and the
answer depends on who is asking. Under the process identity it would report a plan as executable that
the subject may not execute, or prepare a statement against tables the subject cannot see. The check
has to be asked as the same principal as the question, or it answers a different question.

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
to, which makes the audit sink part of the identity path and not an accessory to it.** The `oauth`
skill already states the rule - record the whole principal chain, refusals included, before returning
- and this is why it cannot be satisfied by the data system's log.

**A refusal comes back in the `Ok`.** Minting failures split cleanly: "this subject may not reach
that source" is a governance outcome the caller can be told about, and "the authorization server
returned a 502" is an error. Putting the first in `Err` would let a client library retry a governance
decision until something works, which is the concern `Surface::answer` was already shaped by.

### 4. One subject and one deadline for N legs, by construction

This is the part that answers the sentence in `AGENTS.md` with a mechanism instead of an intention.

The obvious shape is a credential per leg, each carrying its own subject, and a check that they all
match. That is a check: it can be moved, skipped, or written in one of the two places and not the
other. The shape taken instead **hoists every property that has to agree across legs out of the legs
and into the set**:

```rust
/// Everything one answer executes with. One subject, one deadline, N legs.
pub struct LegCredentials {
    /// Written once. N legs cannot disagree about it, because there is one field.
    subject: Subject,
    /// The earliest expiry among everything minted, including the caller's own assertion.
    not_after: Expiry,
    by_source: BTreeMap<SourceName, Presented>,
}

/// What an adapter presents. Two shapes, because two postures are not one field.
pub enum Presented {
    /// A bearer credential the adapter hands to the data system.
    Token { material: Secret },
    /// A principal the data system switches to on a connection the DEPLOYMENT
    /// authenticated. Not a `Secret`: a role name is not secret, and the trust here
    /// is the connection's rather than the subject's. A separate variant so that
    /// weaker posture is a shape a reader can see, never a field on the stronger one.
    Principal { name: PrincipalName },
}
```

`LegCredentials` has no `insert`, no public field and one constructor, in a private module, taking
one `Subject`. So "every leg runs as the same subject" is not enforced by comparing anything. Two
subjects in one answer would need two `LegCredentials` values, and `answer` takes one - the way
`PinnedDefinitions::pin` computes its digest from the definitions it stores rather than accepting one
as a parameter.

**The limit, stated with the claim.** What this makes unrepresentable is *disagreement between legs*.
It does not prove the material in `Presented` actually authenticates that subject at that data
system: a broker adapter that exchanged the wrong token, or mapped a claim to the wrong role, would
produce a perfectly consistent `LegCredentials` for the wrong person. That is the broker adapter's
contract, and the only thing that can check it is *The test that does not exist yet* below. The type
closes the failure mode that federation introduces; the test closes the one the broker introduces.

The same hoist handles the deadline. `not_after` is one field for the whole answer, the earliest
expiry among the exchanged tokens and the caller's own assertion, so there is one thing to check and
no way for two legs to be checked against different clocks.

### 5. Mixed deployments: a posture per source, a requirement per dataset, and a load-time pairing

A deployment is explicitly permitted to hold both impersonating and non-impersonating sources. That
mix is the hardest problem in this record, because the wrong outcome is not a failure - it is an
answer. So it is four typed facts and two checks, and none of them is a sentence anybody has to
remember.

**5a. There are two deployment modes, and they differ in kind rather than in degree.**

**Single-user** means credentials are static configuration: one user, one host, not multi-tenant.
There is no per-request identity to establish, so a non-impersonating source is correct for
**everything** - the one user reads all, by design, and the configured credential is that user's own.
`examples/single-player` is this, and it stays a first-class deployment rather than a degraded one.

**Multi-user** means the caller's identity arrives per request, and a dataset declared subject-only
executes under that caller's own credential. Non-impersonating sources are still permitted, for
non-critical data only.

Stating both is what makes the governing rule precise: **critical data is never served under an
identity that is not the asker's.** In single-user mode that holds trivially, because the configured
credential *is* the one user's - so the rule does not mean a single-user deployment must acquire
per-request impersonation. It means the pairing of data to identity is checked in both modes, and only
one of them has more than one identity to get wrong.

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

**5b. The posture is a property of the source, and it is typed.**

Not a `bool`, and not an `Option` whose absence means the permissive thing. The precedent is already
in the tree: `TlsTermination` is a closed enum whose variants each carry the sentence the startup log
prints, so a deployment cannot be wrong about which posture it has. Identity gets the same shape:

```rust
/// How a source establishes the identity a query runs as. Printed at startup, per source.
pub enum SourceIdentity {
    /// The asker's own credential, minted per request. The only posture a dataset
    /// declared subject-only may be reached through.
    Impersonated,
    /// One identity for every caller. Carries a witness the configuration layer
    /// constructs from a key an operator wrote, so this posture cannot be arrived
    /// at by leaving anything at a default.
    Shared { declared: SharedIdentityDeclared },
}
```

Two things make it safe rather than merely typed. The `Shared` variant carries a witness only the
settings layer can construct, so it is unreachable without an operator writing the key - which is the
`SharedIdentityDeclared` equivalent of the file-backed case the old shape of this part described, and
it subsumes it: a CSV directory is a `Shared` source whose operator said so. And the key is
**required**, so a source that declares no posture is a startup refusal rather than a default. The
settings tree has no `sources:` section at all today, so this arrives with one.

There is a second, orthogonal fact: whether the *adapter* can carry a per-subject credential at all.
That is a property of code rather than configuration, so it belongs on the port and not in the file -
and the cross-check is a startup refusal too: `Impersonated` configured against an adapter with no
support cannot be deployed. Two facts, one check, at the one moment where refusing is free.

`SecuritySettings::describes_identity()` stops being a `const fn` returning `false` and becomes a
summary derived from those postures. **The deployment mode is derived, never declared twice:** every
source `Shared` is single-user mode, any source `Impersonated` is multi-user mode. One owner per
artefact, and the startup log prints the mode and the per-source postures underneath it.

**5c. The requirement is a property of the data, declared in the catalog, on the model.**

Criticality is a fact about data, and data lives in a model - so the declaration goes on the model
rather than on the metric, and a metric's requirement is **derived as the strictest over the models
its plan reads**. That direction matters: declaring it on the metric would let a metric claim it
needs no impersonation while reading a model that does, and a join is exactly how that happens.
Deriving upward from the models cannot produce that hole.

```rust
/// Whether this model's data may be read under an identity that is not the asker's.
pub enum IdentityRequirement {
    /// Only the asker's own credential.
    SubjectOnly,
    /// May be read under a shared identity, because the catalog author said so.
    SharedPermitted,
}
```

Named for what it controls rather than as a classification scheme, deliberately: a vocabulary of
sensitivity tiers would be a second thing to keep in step with whatever the deployment's real one is,
and this field has exactly one consumer.

**The field is required, and old catalogs stop loading.** There is no default, because both defaults
are wrong: defaulting to `SharedPermitted` ships the failure quietly, and defaulting to `SubjectOnly`
makes every existing single-user deployment refuse to start for data that was never sensitive. A
required field turns both into one explicit edit per model, made by the person who knows. That is a
real migration cost and it is the right one - **a load refusal is cheap next to answering critical
data under a service account**, and this repository already refuses a bundle whose anchors do not
hold rather than serving it degraded.

**5d. The pairing is checked at startup, and again at plan time as defence in depth.**

Because the posture is per source and the requirement is per model, and a model names its source, the
pairing is **static**. So the primary check is at startup, over the whole bundle:

> In multi-user mode, a source declared `Shared` that holds a model declared `SubjectOnly` is a
> deployment that **does not boot** - loudly, naming the model and the source.

The precedent is already in the tree and is the right one: `open_engine` refuses before the service
starts on a catalog that spans several data systems, on a source this build has no adapter for, and on
a missing data file. A misconfigured deployment therefore never serves a single question, which is
strictly better than one that boots, passes its probes, and refuses a class of questions for as long
as nobody notices. It extends `verify_and_validate`, already the only constructor of `Validated` and
already taking the warehouse, to take the postures too.

**And there is a plan-time refusal behind it, because the process outlives its configuration.** A
catalog can be reloaded while the process lives, so the startup check is not the last word - and that
is what makes a runtime variant provokable rather than decorative:

| Variant | Status | Why |
| --- | --- | --- |
| `SubjectOnlyDataOnSharedSource { source, model }` | `409` | A conflict between the question and how the deployment is arranged, which is the reasoning `PlanSpansTwoSources` already uses. Refused, never answered with a warning |

It is provoked by a fake warehouse declared `Shared` plus a model declared `SubjectOnly`, with no real
data system - so it satisfies `AGENTS.md`'s rule that a variant no test can provoke is one the enum
refuses to carry, and the test is a per-variant one in the existing refusal corpus.

**5e. Flipping a deployment from single-user to multi-user re-evaluates every pairing.**

Stated as a rule rather than left implicit, because it is the transition that turns a correct
configuration into a dangerous one **unchanged**. A single-user deployment may legitimately hold every
dataset behind non-impersonating sources; the same file in multi-user mode is critical data served
under a shared identity. So: **the mode is an input to the startup check, and changing it re-runs the
whole pairing** rather than any incremental view of it. The startup refusal is most of what makes the
transition safe - a deployment that flips the mode and is now wrong does not boot - and the rest is
that neither the posture nor the requirement has a default, so there is no pairing the check cannot
see.

What this gives `AGENTS.md`'s untested sentence is teeth. "Every leg runs as the same subject, or the
plan is refused rather than downgraded" becomes two checkable halves: every leg's credential comes
from one `LegCredentials`, which has one subject field (part 4); and a leg may run under a shared
identity only where every model it reads was declared to permit that - checked at boot, and again when
a plan is built.

**5f. Provenance records the mode per leg, beside the definition digest and not under it.**

An answer must not be mistakable for impersonated when it was not, so `Provenance` grows a per-source
execution record: which posture each leg ran under, and the subject it ran as.

**Beside the digest, not under it, and the reason is the digest's own purpose.** The definition digest
is over the definitions, so that two deployments serving the same catalog certify the same numbers.
Identity posture is deployment configuration rather than authored content - a different owner - and
hashing it in would make the same catalog produce two digests in two deployments, which is the one
property the digest exists to have. That is also why it differs from the knowledge declaration, which
*is* under the digest: knowledge is content the catalog author wrote, and it changes what the prompt
claims.

**5g. The inference risk, stated without overclaiming.**

A plan may join a leg read as the subject to a leg read under a shared identity. **Defended:** the
subject-only leg's rows are filtered by its own data system, so the joined output is constrained by
what the subject may see on that side, and the shared leg contributes only data a catalog author
declared shareable.

**Not defended, in two parts.** The declaration is a trusted precondition - nothing checks
`SharedPermitted` against reality, exactly as nothing checks a relationship's declared cardinality
against the data, so a mislabelled model is a hole this design cannot see. And the concrete inference
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

| Outcome | Variant | Status | Why |
| --- | --- | --- | --- |
| The subject has no credential at that source | `CredentialUnavailable { source }` | `403` | Understood, and refused. Asking differently does not help; a different subject or a grant does |
| The source is arranged so that nobody can be impersonated on it | `SourceCannotImpersonate { source }` | `409` | A conflict between what was asked and how the deployment is arranged, which is the reasoning `PlanSpansTwoSources` already uses for `409` |
| The broker could not be reached, or answered something unparseable | not a refusal | `503` | `SurfaceFailure`, from the `Err` side. Nothing about the question was wrong |
| The caller's own assertion is expired or invalid | not a refusal | `401` | The transport rejects it before the domain sees a `Caller` at all |

**This is the amendment to ADR 0005**, which states that "the `403`s are not a statement about a
credential" because at the time no token widened anything. `CredentialUnavailable` is one, so that
sentence stops being true the day this lands - and its `detail` must not send a caller looking for a
better deployment token, because what is missing is a grant at the data system.

**An expired token mid-query is deliberately not on that list**, and the reason is worth more than a
variant. Once a leg is running there is nothing for sutura to refuse: the data system decides what
happens to a session or a job whose credential ages out, and we cannot un-authorise work already in
flight. What is available is a floor, checked before the first leg starts, on how much life the
credential has left relative to what the query may take - and that check belongs in the broker
adapter, because it is the only component that has both a clock and the configured query timeout. The
domain reads no clock today, in either crate, and this design does not give it one: `Expiry` is
carried so an audit record can name it, exactly as `TimeRange` carries dates the caller resolved.

Which means `Expiry` is, for now, a field the domain holds and nothing in the domain reads - the shape
`ParamValue::Integer` had before it was deleted, stated rather than hidden. It arrives with the broker
adapter that refuses on it or the audit sink that prints it, and a variant for "expiring too soon"
arrives the same way: a fake broker can provoke the two variants above from the day the port exists,
and could not provoke that one.

### 7. `Secret` composes; it does not grow an expiry

`Secret` is unchanged: one property - opacity - an infallible constructor because every string is a
valid secret, a hand-written redacting `Debug` and `Display`, and no `PartialEq`, so `==` on
credential material does not compile.

An expiring token is not a secret with a date on it. Adding `not_after` to `Secret` would put an
invariant on a type whose constructor correctly has none, and would make the field either optional - a
pre-shared bearer token does not expire - or a lie for the values that have none. So the expiry lives
one level up, on `LegCredentials`, hoisted where it belongs anyway, and `Presented` holds a plain
`Secret`.

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
prefer a source whose can.

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

## The test that does not exist yet

`AGENTS.md` says this test cannot be written until a credential exists per leg. Naming it precisely
is what makes the invariant real later, so here it is named, including the parts that stop it from
being a test that passes for the wrong reason.

**PostgreSQL first, not BigQuery.** The fixture has to be one CI can stand up with no cloud account
and no tenant, and the enforcement has to be the data system's rather than ours.

**The fixture.** One table with a column the policy discriminates on, row-level security enabled
**and forced**, so the table's owner does not bypass its own policy - a fixture where the owner is
exempt is a fixture that proves nothing. Two roles, neither of them the owner, neither holding the
attribute that bypasses row-level security, both granted `SELECT`. A policy that reads the identity
in effect rather than a session variable the client sets, because a variable the client sets is our
own claim rather than the data system's check.

**The two identities.** Two subjects whose validated claims map to those two roles. Not one subject
asked twice, and not two subjects mapped to the same role: the assertion is about two identities
seeing two things.

**What it asserts**, in the order that matters:

1. The same certified question - same metric, same grain, same range, no dimensions - asked as each
   subject returns **different** rows, and each equals what that role is permitted to see.
2. A third run under the deployment's own identity returns **more** rows than either subject. This is
   the assertion that catches a fixture whose policy is not actually on, which is the way this test
   fails to a false green.
3. The negative control: the same fixture with row-level security disabled gives both subjects the
   **same** rows. A test that cannot distinguish the two states is not measuring the two states, and
   this is what makes the first assertion red before the mechanism exists and green after.
4. A connection borrowed from the pool after a subject's query reports the pool's own identity and
   not the previous subject's. This is the dirty-connection case, and it is the worst outcome this
   system can have: an answer that looks correct and was read as the wrong person. On PostgreSQL the
   assertion is specifically that `SET ROLE NONE` and not `RESET ALL` is what the check-in path runs,
   because the second one leaves the role in place - so this case is also the regression test for
   that finding.
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
ones that arrive with the port rather than with an adapter, against fakes, in the existing corpus:

1. Multi-user mode, a source declared `Shared`, a model in it declared `SubjectOnly`: **the service
   does not start**, and the message names the model and the source. Asserted on the startup path, the
   way the anchor failure already is.
2. The same pairing reached at plan time - a bundle swapped after boot - is
   `SubjectOnlyDataOnSharedSource`, and the assertion is on the refusal **and** on the absence of rows.
3. **Single-user mode with every dataset behind non-impersonating sources starts and answers.** This is
   the test that stops the mechanism from being a ban: the permitted deployment has to keep working, or
   the next person deletes the check.
4. **The transition test:** that same single-user configuration, with only the mode flipped to
   multi-user, **fails to start.** One field changes, nothing else, and the outcome inverts. That is
   part 5e as an executable assertion rather than a rule, and it is the one that would catch a future
   incremental check that looked at only what changed.

**What a fake can and cannot do.** The fake broker that ships with the port can provoke both refusal
variants and both `Presented` shapes, which is what makes the refusal corpus complete without a data
system - that is what fakes are for here. It cannot assert 1 through 4: a fake warehouse answering
the same rows for every subject would be a test that looks like coverage and measures nothing, which
`AGENTS.md` says is worse than no test. So the split is deliberate: **the refusal corpus is a fake,
and the two-subject assertion is a real data system in a container, registered in the
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

**One `mint` call per leg.** Rejected because it puts the subject and the deadline in N places, and
RFC 8707 wants N audience-restricted tokens from one decision anyway.

**Waiting for federation to decide any of this.** Rejected. Part 1's signature change is the whole
mechanism, and it is one line on a port today against every adapter and composition root later.

## What has to be true before any of this becomes an invariant

Nothing in this record may be cited as an invariant, and `AGENTS.md`'s Invariants table gets no new
row from it. What would earn one, in order:

| Row it could become | What has to exist first |
| --- | --- |
| A leg cannot execute without a credential | The `Warehouse::execute` signature above, and every adapter compiled against it |
| Every leg of one answer runs as one subject | `LegCredentials` with a private constructor, and a `compile_fail` doctest for the second one, the way `Validated` has two |
| A source that enforces nothing per subject says so at startup | The `sources:` configuration section, the required mode key, and `describes_identity()` deriving from both |
| Critical data is never read as anything but the asker | `IdentityRequirement` on the model as a required field, the startup pairing check, and `SubjectOnlyDataOnSharedSource` with its provoking test |
| An answer cannot be mistaken for impersonated | `Provenance` carrying the per-leg posture, beside the digest, and a golden that shows it |
| A subject that cannot be impersonated is refused, not answered | Both refusal variants, their statuses in `sutura_http::wire::refusal`, and a fake broker that provokes each |
| Two subjects get different rows | The whole test above, against a real data system, in the adapter matrix |

Until then this is a design, and `AGENTS.md`'s *Built And Not Wired* section is the precedent for
what to do with a claim whose mechanism does not exist yet: write it where a reader cannot mistake it
for the table.
