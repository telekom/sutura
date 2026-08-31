---
title: Design principles
description: The long form of the four principles every type, error and boundary here is reviewed against.
---

# Design principles

`AGENTS.md` names the four principles in a line each. This is the long form, and it exists because
"we follow the newtype guide" is not a rule anybody can be held to. Adopted as policy means every
distinction the source draws, including the ones that are inconvenient here.

**Nothing on this page is an invariant, and none of it may be cited as one.** Where a rule below has
a mechanism, that mechanism is a row on the [Invariants](invariants.md) page and is named here
rather than restated, so there is one place to look up what actually fails. Everything else is
**advisory - review catches it or nothing does.** Where a rule says *above*, it means that page.

The last bullet of *Conventions* names three principles in one line each. This is the long form, and
it exists because "we follow the newtype guide" is not a rule anybody can be held to. Adopted as
policy means every distinction the source draws, including the ones that are inconvenient here.

Four sources, and they are the definition of *correct* for a review here:

| Principle | Source |
| --- | --- |
| A newtype parses rather than validates | [the newtype guide](https://www.howtocodeit.com/guides/ultimate-guide-rust-newtypes) |
| An error is a typed enum whose fields carry the context | [structured error handling](https://www.howtocodeit.com/guides/the-definitive-guide-to-rust-error-handling#structured-error-handling-in-rust) |
| Dependencies point inward | [hexagonal architecture in Rust](https://www.howtocodeit.com/guides/master-hexagonal-architecture-in-rust) |
| Security comes out of modelling the domain precisely, not out of a layer on top | secure by design, in the sense Johnsson, Deogun and Sawano give it: domain primitives, invariants enforced at construction, illegal states unrepresentable, failing securely. **No URL, deliberately** - the rules below are derived from those four and from what this repo already does, so read them as ours rather than as a summary of theirs |

**Nothing in this section is an invariant, and none of it may be cited as one.** Where a rule below
has a mechanism, that mechanism is already a row in *Invariants* and is NAMED here rather than
restated, so there is one place to look up what actually fails. Everything else is **advisory -
review catches it or nothing does.** `.agents/skills/engineering/rust/SKILL.md` walks the same
ground mistake-by-mistake, with a *Caught by* column that says *review* wherever nothing fails the
build; this section is the reasoning underneath those tables and does not repeat them.

## Newtypes And Domain Primitives

The whole return on the pattern is the guide's guarantee - *"If an instance of a newtype exists, we
know that it is valid"* - because that is what lets downstream code stop asking. A newtype that can
still be checked has moved the question rather than answered it, and a check that accepts more than
the type's name claims is worse than none - the skill's table carries the real case this repo
already shipped.

- **One canonical constructor, and every other way in delegates to it.** *"Define conversion traits
  in terms of a canonical constructor."* Here that constructor is `parse`, and `From`, `TryFrom` and
  `#[serde(try_from = "..")]` route through it. A second copy of the checks is where the next rule
  fails to get added.
- **`try_from` and `into` are a pair.** `serde(try_from)` affects `Deserialize` only, so a derived
  `Serialize` still writes the struct. `Date` shipped exactly that: a date this workspace serialized
  was a date its own `Deserialize` refused. It mattered because the definition digest is taken over
  the serialized form, so it covered a field layout that appears in no catalog file rather than the
  ISO text an author wrote. Beyond the guide, and learned here.
- **Sanitize, then validate, and do both inside the constructor.** Trim, fold case, strip what is
  invisible, then check the shape - so a derived `PartialEq`, `Hash` and `Serialize` all agree which
  value this is, and no comparison site has to normalise. `Phrase::parse` is the worked example.
  **State the limit with the claim:** normalisation reaches exactly as far as what it names, and
  *Invariants* records that ours stops short of Unicode normalisation.
- **Derive the standard traits where they mean something, and refuse the one that does not.**
  `Debug`, `Clone`, `PartialEq`, `Eq`, `Ord`, `Hash` cost nothing and get used. `Default` is the
  guide's own counterexample: a default email address is not a thing, and a `Default` here would be
  a value that never passed `parse`.
- **Write the comparison by hand when the newtype's ordering is not the inner type's.** Derived `Ord`
  on a struct is declaration order, which is why `Date` documents its field order and asserts it in a
  test: reordering the declaration would silently invert every comparison.
- **`AsRef` yes, `Deref` no, `Borrow` almost never.** `Deref` re-exports the inner API and the
  invariant leaks out with it - that one is in the skill's table. `Borrow` is the one worth adding
  here: the guide calls it *"unofficially unsafe"*, because implementing it PROMISES the wrapper
  hashes, compares and orders identically to what it borrows and the compiler checks nothing - so a
  newtype that folds case where the inner type does not turns a map lookup into a silent miss on an
  entry that is present. *"Scrutinize any `Borrow` implementation you see in code review."* There is
  no first-party `Borrow` impl in this workspace today.
- **Getter names carry the cost.** `as_x` borrows, `into_x` consumes, and a hand-written `to_string`
  shadows the one `Display` already gave you.
- **A mutating method preserves the invariant or does not exist.** The guide's `NonEmptyVec::pop`
  returns `None` rather than emptying the vec, and the payoff is that `last` is infallible. Ours
  mostly sidesteps this: domain values are parsed once and read.
- **Prefer an associated function to an inherent method on a generic wrapper**, so a wrapper method
  and an inner method cannot collide at resolution.
- **The guide's escape hatch does not exist here.** `new_unchecked` is `unsafe`, `unsafe_code` is
  `forbid`, and a crate cannot re-allow it - so re-parsing a value that was already parsed is the
  price of the ban, paid deliberately.
- **No `nutype`, no `derive_more`.** The guide's own caveat is to write the boilerplate by hand first
  and understand what a macro would generate. There is a second reason here: the domain's dependency
  list is four crates and `cargo xtask check-boundaries` walks the whole transitive tree, so a macro
  crate arriving in it is an architecture decision rather than a convenience.
- The orphan rule is a reason to *reach for* a newtype, never the reason to design with them. Type
  safety is.

## Structured Errors

The variant is the contract; the message is not. The reason is Hyrum's Law rather than taste - with
enough callers, every observable behaviour gets depended on, error strings included, and the guide's
example is Go's `http.MaxBytesError`, whose text carries a comment saying it cannot be changed
because something downstream matches on it. So: *"Codify all possible error states in your public
API."*

- **The audience decides the shape.** A caller that must branch gets a typed enum. A dynamic error is
  for the case where nothing but a human will read it, and a caller forced to downcast into your
  types is reading your implementation - the legitimate downcast is a caller retrieving an error it
  handed you itself.
- ***"Return only your own or standard library error types across crate boundaries."*** Mechanised
  for the worst cases: `check-boundaries` fails `Result<_, String>` and a dynamic-error crate in any
  library crate, and `anyhow` appears nowhere in this workspace - `Cargo.lock` included, so not even
  transitively. **Not** mechanised: a variant re-exporting a third-party error type is a review
  question.
- **Erasure has one honest place, and it is a boundary.** `sutura_app::ErasedCause` is a
  `Box<dyn Error + Send + Sync + 'static>`, and it is not a counterexample to the rule above:
  `SurfaceFailure` keeps each cause as an owned `#[source]`, so the chain still walks and a caller
  that knows the adapter can still downcast. `ServiceError<E>` stays generic in the adapter's error
  precisely so the typed error survives to that point. The shape this replaced was a message plus a
  `Vec<String>` of causes - a presentation of an error rather than an error API. Flattening to text
  still happens, at the logging sink, which is the one place text is the point.
- **Narrow, per-operation error types; never one umbrella enum per module.** The guide's rule is to
  prioritise the *relevant* information and minimise unrelated noise: ten variants where two apply
  makes every caller filter. Its own test is the practical one - if enumerating the failure
  permutations is a chore, the type is doing too much. Compose at the boundary instead, with a variant
  for the inner error and `#[from]`.
- **Wire the cause, because nothing does it for you.** `Error::source` defaults to `None`, so a chain
  you did not attach does not exist. `#[from]` or `#[source]`; `.map_err(|_| ..)` throws the cause
  away, and that one *is* caught - `clippy::map_err_ignore`, from the `restriction` category.
- **Errors are `'static` for a reason.** They are handled after the code that produced them returned,
  sometimes on another thread. That is also where `ErasedCause`'s `Send + Sync` comes from: a
  transport answering on a blocking pool sends the failure back across a thread boundary.
- **No catch-all variant.** `std::io::Error` is the guide's cautionary tale: `ErrorKind::Other` became
  load-bearing because callers matched on it, and adding precise variants broke them - the repair was
  a hidden `Uncategorized`. Here the pressure runs the other way, and *Invariants* records it: a
  `RefusalReason` variant no test can provoke is one that enum refuses to carry.
- **An expected outcome is not an error at all.** This is the sharpest departure from the guide's
  framing, which is about what to put in `Err`; the question that comes first here is whether the
  failure belongs there. A governance refusal is `ToolOutcome::Refusal`, in the `Ok`. `Err` is for
  something that went wrong, and a question this deployment declines is something that went right.
  `Surface::answer` is where a transport inherits that, and the reason is in its doc comment: a
  caller must not be able to mistake "you may not ask that" for a hiccup and retry until something
  works.
- **Nothing sensitive in an error.** The typed fields are read by machines and the `Display` by
  humans; neither is a place for a credential, a row, or a path. `Secret` mechanises the credential
  half - see the two rows above, both of which exist because the accident is silent.
- One deliberate deviation, already recorded in the skill: `#[non_exhaustive]` is not used here, and
  `missing_errors_doc` is allowed because an exhaustive typed enum already is the documentation.

## Ports And Adapters

`sutura-domain` is the hexagon's interior; *Layout* above is the map. The guide's line is that the
flow of dependencies points in one direction, towards the domain. `check-boundaries` is the only
mechanism, and it reads **dependency direction** - not intent, and not which crate declares a trait.

- **The domain declares the port, named for what the domain needs**, and an adapter conforms to it.
  `Warehouse`, `SemanticCatalog`, and `Surface` for the driving side. That a driving port is not
  owned by one of its callers is a row above, and that row says plainly it is not gated.
- **The adapter wraps the library and maps its errors at the edge.** *"Wrap external libraries and
  expose only the functionality your application requires."* A `datafusion` or `duckdb` error
  reaching a caller of the port is the failure mode; nothing stops it except the port's signature
  naming domain types and domain errors only.
- ***"Always separate your public errors from their domain representations."*** The public shape here
  is the refusal code on the HTTP surface, and choosing it is the transport's job.
- **Transport adapters stay thin:** parse the wire shape, translate into the domain type, call the
  port, map the outcome back. A predicate assembled in a handler is business logic in an adapter, and
  *Changing The Query Path Or The Tool Surface* is what it would have to get past.
- **Composition happens once** - `sutura-serve` for the HTTP surface, `sutura-cli` for the binary -
  with generics and trait bounds rather than `dyn`. *"The less code you put in `main`, the smaller
  your testing dead zone."*
- **A port gets a fake, and the reason is coverage rather than speed.** Integration tests are not
  suited to exhaustive coverage, and every refusal variant has to be provoked somewhere.
  *Conventions* says fakes, not mocked HTTP; this is why.
- **An adapter never calls another adapter.**
- **No serde on a domain type for a transport's convenience** - a wire shape belongs to the
  transport. The skill records the one exception and why `#[serde(try_from)]` keeps it from being a
  hole.
- **One domain, on purpose.** The guide says start with a single large domain, and that entities
  which must change together in one atomic operation belong in the same one; the tell that a boundary
  is wrong is a transaction leaking into business logic. The equivalent tell here is a question that
  would span three or more sources, refused as `PlanSpansTooManySources` rather than split - exactly
  two is split into legs and combined above them, or refused as `FederationNotExecutable` while no
  selected adapter declares `EXECUTES_LEGS`.
- **Two deviations, both deliberate.** The guide lets `anyhow` flow freely and recommends an
  `Unknown(anyhow::Error)` catch-all in a domain error enum; neither is allowed here, because
  `check-boundaries` fails a dynamic-error crate in a library crate and a catch-all is exactly what
  the refusal enum may not have. And the guide's "do not panic on an unexpected error" is stricter
  here than there: it argues from a poisoned mutex, while shipped profiles compile with
  `panic = "abort"`, so a panic is process death.
- The guide also lists when hexagonal is not worth the tax - a solo project, CRUD with no business
  logic, a path where the transformation cost is the product. This is none of those: the boundary is
  the product, and adding a metadata provider or a data system is a registration rather than a test
  edit, which is a row above.

## Borrowing, And What Deserves An `Arc`

Security and performance are decided together here, and both are decided early. A needless copy on the
federated path multiplies the working set against a memory bound that REFUSES, so an allocation in a
leg is a correctness question rather than a style one.

- **Prefer borrowing. A clone is a decision with a reason, never a way past the borrow checker.** If a
  lifetime is hard, the shape is usually wrong: something is being held across an await it does not
  need to cross, or a value is being owned where a reference would do.
- **`Arc` is for state that is genuinely shared across tasks and immutable once built** - the pinned
  bundle, the certified key the TLS resolver hands out. It is not a lifetime escape hatch, and
  `Arc<Mutex<_>>` around per-request state is the shape to stop and rethink.
- **The scoped view BORROWS the pinned definitions** rather than copying them, and that is not an
  optimisation: it is what keeps `load()` off the request path and makes visibility filtering
  incapable of acquiring I/O.
- **A port takes `&self` and holds no request state.** That is an invariant above, and it is also what
  makes sharing an adapter across tasks free rather than something to engineer.
- **Know which clones are cheap.** Arrow buffers are reference-counted by construction, so cloning a
  batch moves no data; treating it as a copy produces worse code, not safer code. The opposite mistake
  is cloning a `String` per row because the signature asked for one.
- **Measure rather than assert.** The numbers that decided the federation shape were wall clock and
  peak resident set on a real corpus, not reasoning about allocations. A claim about cost in a review
  is worth what its measurement is worth.

## Secure By Design

A control that holds by construction is the only kind this repository counts. The three sections above are
that argument applied to types, errors and boundaries; what follows is the rest of it, and most of it
is a habit rather than a gate.

- **A catalog document is untrusted input, not trusted configuration.** It reaches a parser, and the
  row about the panicking fragment API exists because an abort was reachable from a catalog file.
  Anything read off disk gets the treatment a question off the wire gets.
- **Bound the input at the edge, before anything does work proportional to it.** The transport caps
  the request body from `server.max_body_bytes` and holds a request timeout; the bundle caps authored
  text with `MAX_KNOWLEDGE_BYTES`; a question's range and dimensions are bounded, and so is the
  result. An unbounded input is a denial-of-service primitive whatever else it is, which is why
  **availability is treated as a security property here and not as an operational one.**
- **Order the checks inside a parse for the clearest diagnostic, and say that is what you are doing.**
  The classic ordering - origin, size, lexical content, syntax, meaning - is there so an expensive
  check never runs on input a cheap one would have rejected. Once the input is already bounded, that
  argument is spent, and `NoteBody::parse` deliberately checks *is there prose here at all* before it
  checks bytes, because "this note is empty" is the more accurate thing to tell its author than "this
  note has a hidden character". The source says so at the branch. **A defence and a diagnostic are
  different jobs; do not let a comment claim one and deliver the other.**
- **Prefer unrepresentable to checked.** `TimeRange` has no unbounded form. `Measure` has no
  `expression:` field and no `Option<String>` at any depth. A check can be moved, skipped or ordered
  wrongly; a shape that cannot hold the value cannot.
- **Fail closed on the query path, and state which way each default points.** A refusal the caller
  can see beats a degraded answer it cannot: a result at the row cap is refused rather than truncated,
  a plan over two sources is refused rather than downgraded, and an unvalidated bundle is never
  served. The deliberate opposite lives in the tooling, where `classify` and its siblings **fail
  open**, because the expensive failure there is a new directory silently skipped rather than a wasted
  minute. **Neither direction is the default: what a wrong answer costs decides it, per mechanism,
  written down where the mechanism is.**
- **A credential does not travel through a log, an error or a `Debug`.** `Secret` is the mechanism,
  and it is two rows rather than one because there are two silent accidents - a redaction that only
  holds at the top level, and a derived comparison that becomes a timing oracle at whatever call site
  adds it later. **`docs/adr/0020` turned the first of those from a redaction into a missing impl**,
  and it is the worked example for *prefer unrepresentable to checked* on this page: nothing had ever
  leaked, and the defect was that `tracing::info!(%token)` compiled and printed `REDACTED` where its
  author believed a value had been logged. A check can be skipped; a shape that cannot hold the value
  cannot. The price was one lockfile entry, and the *Layout* table names it.
- **Least authority on the execution leg.** End-to-end impersonation is the point of the product: a
  query executes as the subject who asked it. Where that is not true yet, the guidance says so - and it
  is worth being precise about which half exists now. **Leg 1 is built**: a deployment that declares
  `security.inbound` knows who is asking, from a signature. **Leg 2 is half built, and the halves are
  worth telling apart because only one of them is what the product promises.** The PORT is built: a
  credential per leg exists, `Warehouse::execute` cannot be called without one, and a subject with no
  credential at a source is refused rather than answered as the process - so there is no longer a
  signature that runs as this process, which is what the fallback used to be. **The one method that runs
  with no credential is the boot path's, `Warehouse::verify_anchor`, and what keeps it there is a LINT
  rather than its input type - two reviews to get that sentence right, and the first version of it was
  false.** `clippy.toml` bans the method and `sutura_app::verify_anchors` holds the single `#[expect]`,
  so a second call site is an error under `-D warnings`; a lint reaches this workspace and an
  `#[allow]` walks past it, which is the limit. Its `AnchorPlan` input is a **self-check on that one
  caller**, reading the metric's definition, its anchor's range and its coarsest grain off the pinned
  bundle - so it catches a boot path that compiled the wrong question and it is NOT a barrier: every
  value it reads is publicly constructible. Do not cite the type as a control. What is NOT built is a
  source a deployment SERVES that executes as the asking subject, and the reason moved from the adapter
  to the composition: `sutura-exec-bigquery` declares `PerSubjectCredential` and sends the asker's token
  as its job's bearer, and `sutura_exec_bigquery::WorkloadIdentityBroker` is a broker that really
  exchanges - but no served source is opened against it, `build_bigquery` refuses that posture by name,
  and the broker `sutura-config` ships mints from configuration and performs no exchange. The engine
  still declares it has nowhere for a per-subject credential to arrive. So a deployment can now name the subject in every audit record, record which
  posture each leg ran under, and still read every row as one identity - which is the confusion
  `docs/adr/0014` and `docs/adr/0010` both warn about, and why the startup log prints the limit beside
  the mode rather than only the mode.

  **A scope narrows the SURFACE and it is not leg 2 arriving early.** `sutura_app::Capability` gives a
  deployment two grants to hand out, so a caller can be allowed to read the catalog and not to ask a
  question - which is least authority over the operations, and it is worth having. It buys nothing at
  all over the rows: both operations read the same bundle and every question runs with whatever access
  the process already had. Describing scope filtering as per-caller access would be exactly the
  overstatement the row above exists to name.
- **State the limit next to the claim.** The strongest habit in this repository's prose, and the easiest to lose:
  the leak guard cannot catch a paraphrase, catalog cardinality is a trusted precondition nothing
  checks against the data, and parse-checking a statement is narrower than a data system accepting
  it. A control described as stronger than it is spends trust a reviewer needed elsewhere, so **an
  overstated claim is itself the defect** - and *Built And Not Wired* is what it looks like when we
  find one and refuse to leave it in the table.
