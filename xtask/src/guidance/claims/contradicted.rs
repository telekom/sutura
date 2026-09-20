//! Every claim the tree contradicts, as data.
//!
//! **Split out of `claims.rs` under the 1000-line cap, and the seam is the one the parent named
//! before it was reached:** `CONTRADICTED` is what grows by ENTRY - one claim is about twenty
//! lines - so a file holding the table AND the mechanism AND the tests runs out of room on the
//! entry that finds the next stale sibling, which is precisely when nobody wants to be splitting
//! a module. The table is data and reads nothing; [`super`] is the mechanism that walks it.
//!
//! The tests stay in the parent, beside the mechanism, for the reason its header gives: a file
//! that adds no `#[test]` is one the causality gate may revert, and reverting this one would take
//! the parent's `mod contradicted;` with it.

use super::{Contradicted, Evidence, Withdrawn};

pub(in crate::guidance) const CONTRADICTED: &[Contradicted] = &[
    Contradicted {
        name: "additional named credential writes are held only by review",
        wordings: &["for the CI key and by review for the two principal keys placed beside it"],
        evidence: &[Evidence {
            path: "xtask/src/venues/acceptance/properties.rs",
            holds: "let mut placed: BTreeSet<&str> = commands",
        }],
        instead: "`just hygiene` checks recognised redirects from secret-naming commands and \
                  requires each destination in a cleanup command's argument list; \
                  `xtask/src/venues/acceptance/properties.rs` holds that scan. Indirect copies, \
                  working-directory changes and whether cleanup executes remain review's",
        only: &[],
        except: &[],
    },
    Contradicted {
        // Issue 312, and the reason it is an entry rather than a rewrite alone: the wording was a
        // page's YAML `description`, which mkdocs-material renders into `<meta name="description">`
        // - so a reader who never opens the record is told a DataHub deployment carries no metric
        // layer at all. It was true when the spike was written and the record's own amendment
        // spent it, which is the shape a ratchet is for: the sentence is corrected in place, and
        // this is what stops the pre-amendment summary being reinstated by whoever reads the body
        // above the amendment.
        name: "DataHub can carry no part of a metric",
        // ONE wording, and the whole absence list rather than a phrase out of it: every clause is
        // false for the same reason, and half of it is not a sentence anybody wrote.
        //
        // NOT registered, and worth naming rather than leaving to a reader: the adapter's own
        // crate header states the same list with its hedge attached to the measure clause and
        // the may-provide explanation further down, so it is ambiguous rather than false and a
        // literal against it would fail this gate over prose that carries its own correction.
        wordings: &[
            "carries no measure this repository will execute, no reliable cardinality, no definitional \
             filter, no grain, no value allowlist and no anchor",
        ],
        // The adapter's declaration, not the record's prose. `DefinitionKind::Anchors` appears once
        // in that file and inside `and_may_provide`, so its presence refutes the last clause; if it
        // ever moved to the unconditional list the clause would be more false rather than less,
        // which is why one token is enough evidence here.
        evidence: &[Evidence {
            path: "crates/sutura-catalog-datahub/src/lib.rs",
            holds: "DefinitionKind::Anchors",
        }],
        instead: "the record's own `Amendment, 2026-09-02` moves every one of those to a \
                  declared-and-empty may-provide kind, so a DataHub deployment that defines the \
                  structured property carries the metric whole and one that does not still loads. \
                  `crates/sutura-catalog-datahub/src/lib.rs` declares which kinds those are, and a \
                  page restating them is a second copy of that declaration",
        only: &[],
        except: &[],
    },
    Contradicted {
        // The wording is the one that was actually WRITTEN, in two documents, and not a paraphrase
        // of the claim: the first draft of this entry registered `nothing runs it in CI yet`, which
        // lives in a `.rs` file this check's scope never reaches, and a sentence nobody wrote. That
        // is a rule against nothing plus a page permanently exempt from it - the pair
        // `a_page_a_rule_exempts_holds_a_wording_that_rule_forbids` exists to refuse.
        name: "the default-feature lane has no lint half in CI",
        wordings: &["nothing at all for the lint half", "nothing for the lint half"],
        evidence: &[Evidence {
            path: ".github/workflows/ci.yml",
            holds: "nix run .#default-features",
        }],
        instead: "the required `ci` job runs `nix run .#default-features` on every pull request that \
                  touches Rust, so the lint half of that lane is a required check rather than the \
                  four `cross` link builds alone",
        only: &[],
        // The record states the limit and amends it in place, which is what `except` is for.
        except: &["docs/adr/0017-what-a-bigquery-test-runs-against.md"],
    },
    Contradicted {
        // RENAMED from "there is no HTTP surface". The entry always held both halves of one claim,
        // and a name that said only HTTP was the label version of the defect
        // `github.com/telekom/sutura#241` reports: this entry's own remedy went on saying the agent
        // surface was absent after `github.com/telekom/sutura#189` shipped it.
        name: "a transport surface is absent",
        wordings: &[
            "neither an MCP nor an HTTP surface",
            "The MCP and HTTP surfaces",
            "no MCP server and no HTTP surface",
            // THREE spellings of the surviving half, each found in the tree rather than imagined,
            // per the doctrine on this field. Two of them differ only in case because the match is
            // case-sensitive and a bullet heading capitalises; the third is the one this file's own
            // remedy carried, which is what makes `remedies_hold` a mechanism here rather than a
            // note.
            "no MCP surface",
            "No MCP surface",
            "the MCP surface is absent",
        ],
        evidence: &[
            Evidence {
                path: "crates/sutura-http/Cargo.toml",
                holds: "",
            },
            // `sutura-serve`'s own manifest before `github.com/telekom/sutura#685` step 2 folded
            // that crate into this one's `serve` module - the HTTP surface's composition root is
            // this file now, and its own existence is the same evidence the deleted manifest was.
            Evidence {
                path: "crates/sutura-cli/src/serve.rs",
                holds: "",
            },
            Evidence {
                path: "docs/serving.md",
                holds: "",
            },
            // The agent half, pinned the way the HTTP half already was: the crate, and the task
            // that drives it end to end. The remedy below rests on both, so the day either goes the
            // rule retires and the evidence test names it instead of the sentence going quietly
            // stale.
            Evidence {
                path: "crates/sutura-mcp/Cargo.toml",
                holds: "",
            },
            Evidence {
                path: "justfile",
                holds: "mcp-e2e:",
            },
        ],
        instead: "both surfaces ship. `sutura-http` serves the transport `sutura serve` composes \
                  and `sutura-mcp` serves the agent surface over the process's own pipes, `just \
                  mcp-e2e` drives that one end to end against committed schema snapshots, \
                  `docs/serving.md` is published, and `examples/single-player` has a captured \
                  session. The clause that is still load bearing is the identity: the HTTP bearer \
                  token authenticates the DEPLOYMENT and not the caller unless the deployment \
                  declares `security.inbound`, and the agent surface has no header a token could \
                  arrive in at all - see `docs/concepts.md`",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "DuckDB is the only data system",
        wordings: &[
            "DuckDB is the one adapter that exists",
            "the only data system adapter is DuckDB",
            "a `DuckDB` file",
        ],
        evidence: &[
            Evidence {
                path: "crates/sutura-exec-datafusion/Cargo.toml",
                holds: "",
            },
            Evidence {
                path: "crates/sutura-cli/src/sources/files.rs",
                holds: "DataFusionWarehouse::new",
            },
        ],
        instead: "the engine ships. `sutura query` opens a `DataFusionWarehouse` over the CSV and \
                  Parquet files in the directory it was given, Parquet preferred; \
                  `sutura-exec-duckdb` is a DEV-dependency of `sutura-app`'s tests. \
                  `docs/architecture.md`'s table is the inventory",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "nothing here logs",
        wordings: &["no logging dependency at all"],
        evidence: &[Evidence {
            path: "crates/sutura-runtime/src/telemetry.rs",
            holds: "",
        }],
        instead: "`sutura-runtime` installs a tracing subscriber and three crates depend on \
                  `tracing`. The true half of the claim is the one `docs/concepts.md` makes: a \
                  refusal can be LOGGED and a log line is not an audit record, because nothing \
                  correlates it to a caller there is no type for",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "the measure vocabulary is three sibling shapes",
        wordings: &[
            "one of three closed shapes",
            "a conditional count or a ratio of two aggregates",
        ],
        evidence: &[Evidence {
            path: "crates/sutura-domain/src/measure.rs",
            holds: "pub enum Term",
        }],
        instead: "two shapes over a `Term` of two terms. `docs/adr/0002` records the reversal under \
                  *Two levels, not three siblings* - a conditional count had to be usable as HALF \
                  of a ratio - and `docs/concepts.md` states it correctly. This rule's evidence is \
                  `Term` existing, so renaming that type retires it rather than breaking it",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "no crate declares a feature",
        // TWO wordings, and the second is the whole point of this edit. The first caught one file
        // while the claim lived in three: `justfile` and `devenv.nix` both spelled the PARAPHRASE
        // - "no crate declares a feature", with no `[features]` in it - and the skill page
        // corrected itself in prose while both siblings survived, which is exactly the cause this
        // module's header names. `justfile` has no extension and is outside this module's scope
        // whatever is listed here, so it stays a hand fix held by review; `devenv.nix` is in
        // scope, which is what makes the second wording a mechanism rather than a note.
        wordings: &["no crate declares a `[features]` table today", "no crate declares a feature"],
        evidence: &[Evidence {
            path: "crates/sutura-http/Cargo.toml",
            holds: "[features]",
        }],
        // Deliberately no count and no list: `grep -rln '^[features]' --include=Cargo.toml .`
        // answers both, and a copy of it here is a second thing to keep true.
        instead: "crates here declare features and make dependencies optional. The habit the \
                  passage is about - every entry point passing `--all-features` - is load bearing \
                  now rather than cheap foresight, which is a better version of the point",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "the prompt renders only what the catalog endpoint renders",
        wordings: &["renders exactly what `GET /v1/catalog` renders and not one field more"],
        evidence: &[Evidence {
            path: "crates/sutura-app/src/prompt/knowledge.rs",
            holds: "",
        }],
        instead: "the knowledge layer added four sections `CatalogBody` has no field for - the \
                  glossary, the terms recorded as NOT defined, what the deployment records about \
                  its definitions, and the worked questions - plus caveats under each metric. The \
                  true claim is the one the test asserts: no model, table or column name gets out",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "one write scope in the workflows",
        // ONE wording, and the second one is missing for a reason worth knowing. `ci.yml` carried
        // the same false claim across two lines of a `#` comment block, so its flattened form has
        // a `#` in the middle of the sentence and no needle spells it - see `flatten`. That copy
        // was corrected by hand and this rule cannot hold it.
        // "anywhere" is doing the work: the NARROW claim - the only write scope on a job a pull
        // request can reach - is true, and is what the corrected sentence says. A needle of just
        // "the only write scope" would forbid the true sentence along with the false one.
        wordings: &["only write scope anywhere"],
        evidence: &[Evidence {
            path: ".github/workflows/cache-prune.yml",
            holds: "actions: write",
        }],
        instead: "eight, across five workflows - `cache-prune.yml`, `docs.yml`, `release.yml`, \
                  `release-performance.yml` and `version-bump.yml`. The narrow claim is true and is \
                  the one `ci.yml` makes: the only write scope a pull request can reach",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "the example asserts four measure sets",
        wordings: &["four exact sets"],
        evidence: &[Evidence {
            path: "crates/sutura-cli/tests/example.rs",
            holds: "the example no longer declares both meanings of a zero denominator",
        }],
        instead: "five: shapes, terms, the terms a ratio holds, the aggregates, and both meanings \
                  of a zero denominator",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "query builds a database",
        wordings: &["the database is built in memory"],
        evidence: &[Evidence {
            path: "crates/sutura-cli/src/sources/files.rs",
            holds: "attach_parquet",
        }],
        instead: "there is no database. The engine registers one file per model in process, \
                  Parquet preferred over CSV, and reads it where it lies",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "a served DataHub deployment",
        // The wording that would be FALSE today: no build in this repository can open
        // `catalog.kind: datahub` (the crate is a `sutura-app` dev-dependency and `sutura serve`
        // refuses the kind by name without the `datahub` feature), so a README or record that
        // presents the DataHub deployment shape as a SERVED deployment contradicts the tree. The
        // entry is a RATCHET rather than a repair: it forbids the availability wording wherever it
        // appears, and it deliberately does NOT forbid the shape wording, which is the honest way
        // to describe a deployment nothing can open by default. `docs/adr/0016` and the *Built and
        // not wired* register carry the limit itself.
        wordings: &[
            "a served deployment with per-caller identities",
            "a served deployment whose semantic catalog is DataHub",
        ],
        evidence: &[Evidence {
            path: "crates/sutura-cli/src/serve/catalog.rs",
            holds: "CatalogKind::Datahub =>",
        }],
        instead: "write the deployment SHAPE, whose runnable proof is test code over a recorded \
                  fixture: no binary links the adapter by default, and `sutura serve` refuses the \
                  kind by name without the `datahub` feature, which the *Built and not wired* \
                  register in `.agents/skills/sutura/query-surface/SKILL.md` records",
        only: &[],
        except: &[],
    },
    Contradicted {
        name: "no configuration selects a data system",
        // **This entry exists because review found the gate blind to its own class.** A front-door
        // page - `docs/index.md` - still asserted a compile-time decision after
        // `github.com/telekom/sutura#121` made it a declaration, while the identical sentence had
        // been corrected in `docs/adr/0016` in the same change. A wording is added here the moment
        // it is found, which is what stops the next page drifting the same way.
        wordings: &[
            "no configuration that selects a metadata provider or a data system",
            "there is no configuration that selects one",
            "no configuration that selects a data system",
            "which data system is opened is a compile-time decision",
        ],
        evidence: &[
            Evidence {
                path: "crates/sutura-config/src/sources.rs",
                holds: "pub enum SourceKind",
            },
            Evidence {
                path: "crates/sutura-cli/src/sources.rs",
                holds: "SourceKind::Files",
            },
            Evidence {
                path: "crates/sutura-cli/src/serve/kind.rs",
                holds: "SourceKind::Files",
            },
        ],
        instead: "`sources.<alias>.kind` selects the adapter, and BOTH composition roots dispatch \
                  it through an exhaustive match. What stays a compile-time decision is which KINDS \
                  a given build linked: `kind: bigquery` on a build without the default-off \
                  `bigquery` feature is a refusal naming that feature. `docs/architecture.md`'s \
                  table is the inventory",
        only: &[],
        except: &[],
    },
    Contradicted {
        // **The shape this whole table is for, caught late and worth naming as a class:** a limit
        // whose evidence is *nothing in the tree does X* is only as good as a search somebody ran.
        // This one was written from recollection into `docs/adr/0017`'s fifth amendment and merged
        // 2 h 10 min after `github.com/telekom/sutura#97` had put `id-token: write` in
        // `release.yml`, so it was false on the day it landed and no gate could have known.
        name: "no workflow mints an OIDC token",
        // TWO halves of one sentence, because they fail differently. The first is a claim about the
        // tree and is refuted by the evidence below. The second is what made it a SIGNAL - a reader
        // was handed the permission's arrival as the diff to watch for, and it had already arrived,
        // so the sentence would have gone on reading as an instruction long after it stopped being
        // one.
        //
        // Both needles start INSIDE the sentence, and that is measured rather than tidy: the match
        // is case-sensitive, and `the moment that changes ...` - the wording as merged, mid-sentence
        // after a semicolon - was verified to walk straight past the same sentence written as `The
        // moment that changes ...` at the start of one. Dropping the leading words catches both
        // capitalisations with one needle, which is what `no MCP surface` above pays two for, and it
        // catches `nor a Google` and `nor any Google` - the merged form and the way the sentence is
        // told elsewhere - without asserting that both are in this tree's history. Measured
        // 2026-09-03: `git log --all -S 'any Google auth action'` finds only this entry.
        wordings: &[
            "Google auth action appears in any workflow",
            "moment that changes is a workflow diff with no key to rotate",
        ],
        evidence: &[Evidence {
            path: ".github/workflows/release.yml",
            holds: "id-token: write",
        }],
        instead: "`.github/workflows/release.yml` has held `id-token: write` since \
                  `github.com/telekom/sutura#97`, for keyless signing, with no Google in it at all - \
                  so the permission was never the signal. What is greenfield is the Google half, and \
                  the signal is a Google STS exchange in the acceptance job with no key beside it, \
                  which `cargo xtask check-venues` reads out of `.github/workflows/ci.yml`",
        only: &[],
        // The record that quotes the wrong sentence in order to correct it - the only copy left in
        // the tree, and the reason `a_page_a_rule_exempts_holds_a_wording_that_rule_forbids` exists:
        // an exemption is also a blind spot, so it has to keep earning itself.
        except: &["docs/adr/0017-what-a-bigquery-test-runs-against.md"],
    },
    Contradicted {
        // The TEST half of the default-feature lane, whose CI venue arrived with
        // `check-default-feature-tests`. Registered rather than only corrected in place, because
        // the wording was in FOUR documents by the time it stopped being true - two manifests and
        // two skill pages - and one file fixed while its siblings were not is the whole reason this
        // table judges a CLAIM instead of a line.
        //
        // The scope reaches all four of those documents, manifests included - `run`'s filter is
        // `md`, `nix`, `yml`, `yaml`, `toml`, `sh`. **What it does not reach is the argument
        // itself:** `.rs` is deliberately outside that filter, so the module headers in
        // `default_features.rs` and `default_feature_tests.rs` are where this claim is stated at
        // length and are held by review, for the reason `run`'s own comment gives.
        name: "the shipped feature set's lane is a developer lane with no CI venue",
        wordings: &["it is a DEVELOPER lane", "it is a developer lane"],
        evidence: &[Evidence {
            path: ".github/workflows/ci.yml",
            holds: "nix run .#default-feature-tests",
        }],
        instead: "one half of that lane is a required check. The `ci` job runs \
                  `nix run .#default-feature-tests` on every change the classification marks as \
                  Rust, so the shipped set's TESTS are gated in CI as well as in `just gates`, and \
                  `xtask/src/default_feature_tests.rs` carries the measurement and every limit. \
                  What reaches `just gates` alone is the COMPILE and LINT halves - \
                  `cargo xtask check-default-features` - for which CI still has the four `cross` \
                  link builds and, for the lint half, nothing",
        only: &[],
        except: &[],
    },
    Contradicted {
        // `github.com/telekom/sutura#370` row A, widened by issue 113. The first wording sat on the
        // doc comment of the very method the federated answer path calls, and `just api` republished
        // it at `docs/api/sutura-domain.md`, which `mkdocs.yml` puts in the nav. **Every wording here
        // was TRUE when it was written**, so nothing here could have caught them going false - that
        // direction is `absences`, one module over. This is the other one: a ratchet, so no spelling
        // of the pre-federation belief can be reinstated by whoever reads one file and not its caller.
        //
        // RENAMED from "nothing constructs a second leg", which was the label version of the same
        // defect this table exists for: the entry always held one CLAIM - that a question is not
        // answered across two data systems - and a name naming one of its sentences is how a sibling
        // wording gets filed as a second entry nobody adds.
        name: "this deployment does not answer across two data systems",
        // The other half of the leg sentence is deliberately not here: `there is no combiner` is also
        // how the BigQuery adapter's refusal says a LEG arrived with nothing above it to group it - a
        // different claim, and correct. Registering it would fail correct prose, which is the pair
        // `a_page_a_rule_exempts_holds_a_wording_that_rule_forbids` refuses. What survives is the
        // clause that carries the workspace-wide claim.
        //
        // **The sibling sentences are NOT fixed by this entry, and saying so is the point.** The
        // same absence is written in several more `.rs` doc comments, which this table's scope
        // never reaches - `github.com/telekom/sutura#404` measures each one and says which
        // production line refutes it. They are filed rather than folded in here because one file
        // is an adapter another change owns and one is a record that would need an amendment.
        wordings: &[
            "Nothing constructs a second leg",
            // Issue 113, and the SAME claim rather than a second one: the plan-stage rule went out
            // with the splitter, and this is the sentence that outlived it on a published page and
            // in a record. `docs/concepts.md` was corrected and its two siblings were missed, which
            // is this table's founding cause verbatim. The lineage aside is registered in both its
            // spellings, because the shorter one also opens a sentence that goes on to explain
            // federation - and that one is correct.
            "The plan names exactly one source, so a question that would need two identities is \
             refused before anything runs",
            "a plan resolves to one source, a measure reads",
            "plan resolves to exactly one source, a measure reads",
        ],
        // The combiner's declaration, which is what the sentence says does not exist. It is called
        // from `sutura_app`'s federated path, but the declaration is the narrower fact and the one
        // that retires the rule if federation is ever taken back out.
        evidence: &[Evidence {
            path: "crates/sutura-domain/src/plan/federated.rs",
            holds: "pub fn combine(",
        }],
        instead: "`sutura_app`'s federated answer path builds the second leg and \
                  `crates/sutura-domain/src/plan/federated.rs` declares what groups the \
                  two, so the record's shape is settled rather than provisional. A plan names one \
                  data system per LEG and at most two legs - three or more refuse as \
                  `PlanSpansTooManySources` - and whether the two legs would decide identity the \
                  same way is not a plan-stage fact at all, since a posture belongs to an opened \
                  adapter; that is refused above the credential mint. `docs/concepts.md` carries \
                  the corrected wording and its limits",
        only: &[],
        except: &[],
    },
    Contradicted {
        // `github.com/telekom/sutura#370` row B, and the one shape this table holds that no reader
        // could: the sentence names a type that has NEVER been declared here, so there was no
        // moment it went false and nothing to derive it from. Two skills carried it, which is this
        // table's founding cause verbatim - a correction that lands in one document and is not
        // carried to its sibling.
        name: "the request path reads a scoped view of the definitions",
        wordings: &[
            "dimension validation reads the pinned definitions, not the scoped view",
            "The scoped view BORROWS the pinned definitions",
        ],
        // What the request path actually borrows: the pinned bundle itself, handed out whole by
        // the transport's shared state. A view between the two would be this accessor's return
        // type, and it is not.
        evidence: &[Evidence {
            path: "crates/sutura-http/src/state.rs",
            holds: "fn definitions(&self) -> &sutura_domain::pinned::PinnedDefinitions",
        }],
        instead: "`load()` runs at boot and the request path borrows the pinned bundle whole - \
                  `crates/sutura-http/src/state.rs` hands a handler `&PinnedDefinitions` - so \
                  there is no per-request view over it, and nothing on that path can acquire I/O",
        only: &[],
        except: &[],
    },
    Contradicted {
        // An ENTRY and not a rewrite alone, because the page that carried this is the one an agent
        // debugging a red test is ROUTED to, and it handed them a cause removed in the same commit
        // that edited the paragraph. Three wordings false in the safe direction; the fourth - the
        // `removed BEFORE` clause - false in the UNSAFE one, promising a fail-closed file where a
        // neighbour's entry now deliberately survives.
        name: "a `dev-up` rewrites the whole discovery document",
        wordings: &[
            "still serialises the whole document",
            "so a nix tier's entry goes with it",
            "the next `just test` still answers nothing",
            "removed BEFORE the tier is touched",
        ],
        evidence: &[Evidence {
            path: "dev/src/discovery.rs",
            holds: "fn a_second_provisioners_entry_survives_a_publish",
        }],
        instead: "both writers of the discovery document merge per ENTRY, so a neighbour's entry \
                  outlives a `just dev-up` and keeps the file alive with it - which narrows the \
                  fail-closed claim to one provisioner's own entries. `dev/src/discovery.rs` and \
                  `.agents/skills/sutura/query-surface/SKILL.md` state it correctly",
        only: &[],
        except: &[],
    },
    Contradicted {
        // github.com/telekom/sutura#159. `BigQueryWarehouse::IMPERSONATION` moved from
        // `NoPlaceForASubject` to `PerSubjectCredential` and the correction landed in the crate's
        // own module doc (`crates/sutura-exec-bigquery/src/lib.rs`) but never reached five prose
        // sites - three in 0017 and two in 0018, the second 0018 site (`:141`) phrased differently
        // enough to need its own wording - which is the exact shape this table exists for: one
        // correction, N sibling documents, and the sibling that was missed is a failure rather
        // than a survivor.
        //
        // REVIEW #667: this entry's own `instead` repeated a second stale claim - "a broker that
        // mints a per-leg credential is still unbuilt" - trusted from the same stale module doc
        // (`lib.rs:56-63`, last touched 2026-08-31 in #93, before #284). The broker IS built:
        // `crates/sutura-exec-bigquery/src/sts.rs`'s `WorkloadIdentityBroker` performs the
        // exchange, and `crates/sutura-cli/src/serve/broker.rs` composes it (`build_broker`,
        // #284; that path was `crates/sutura-serve/src/broker.rs` before `github.com/telekom/
        // sutura#685` step 2 folded the crate in). The limit that stays true now is
        // `.agents/skills/sutura/identity/SKILL.md`'s own row: built, though no served binary has
        // executed as a caller yet (`docs/where-identity-is-proven.md`).
        name: "the BigQuery adapter has no place for a subject",
        wordings: &[
            "IMPERSONATION` still reads `NoPlaceForASubject`",
            "IMPERSONATION` is `NoPlaceForASubject` and the crate says so",
        ],
        evidence: &[Evidence {
            path: "crates/sutura-exec-bigquery/src/lib.rs",
            holds: "ImpersonationCapability::PerSubjectCredential",
        }],
        instead: "`BigQueryWarehouse::IMPERSONATION` is `ImpersonationCapability::PerSubjectCredential`, \
                  so a source declared `impersonation-at-source` can be opened here and the posture \
                  cross-check no longer refuses it by name. The broker is built too: \
                  `crates/sutura-exec-bigquery/src/sts.rs`'s `WorkloadIdentityBroker` performs the \
                  exchange and `crates/sutura-cli/src/serve/broker.rs` composes it. What is still \
                  true is narrower - proven by a hosted run whose job held both principals' own \
                  keys, so it resolves per subject and no served binary has executed as a caller \
                  yet (`docs/where-identity-is-proven.md`)",
        only: &[],
        // Both records state the old value and amend it in place, per this repository's own rule
        // for a record: preserve the sentence and correct it beside itself. Excepting them is what
        // stops this entry firing against its own fix; every OTHER page is still held to it.
        except: &[
            "docs/adr/0017-what-a-bigquery-test-runs-against.md",
            "docs/adr/0018-what-the-bigquery-wire-is-built-from.md",
        ],
    },
    Contradicted {
        // telekom/sutura#376. `docs/where-identity-is-proven.md`'s bq-test venue moved from
        // `wired` to `yes` after a hosted run (35076526218). The wording it retired - the broker
        // was built but never taken to a real exchange - fell with it; this row is the ratchet that
        // refuses it coming back anywhere. The two ADR records that used to echo the old wording
        // were amended in place on 2026-09-16 with the superseding sentence, so nothing is
        // excepted to grandfather them. The run held both principals' own keys by construction, so
        // it proved the STS/`iamcredentials` mechanics resolve per subject; the served half stayed
        // unproven.
        name: "the BigQuery exchange never ran against a real STS",
        wordings: &[
            "wired in serve, not proven live",
            "no exchanged token has ever run against a real STS",
        ],
        // Refuted by the venue row's own yes: while the page carries the observed run the rule is
        // live and forbids its wording; if the row ever reverts to `wired` the rule retires itself
        // rather than forbidding a sentence that has become true again.
        evidence: &[Evidence {
            path: "docs/where-identity-is-proven.md",
            holds: "The state is `yes`",
        }],
        instead: "`docs/where-identity-is-proven.md` records a hosted `workflow_dispatch` of \
                  `bigquery-exchanged-identity` that exchanged each principal's job-time assertion \
                  against a real STS and resolved it to that principal's own account. The job held \
                  both principals' own keys by construction, so the mechanics resolve per subject \
                  and no served binary has executed as a caller yet",
        only: &[],
        except: &[],
    },
    Contradicted {
        // github.com/telekom/sutura#159. `AGENTS.md` carried a *Built And Not Wired* section that
        // registered every claim with no mechanism behind it; the register MOVED to
        // `.agents/skills/sutura/query-surface/SKILL.md` under the router rewrite, renamed to
        // lowercase on the way (*Built and not wired*). Four ADRs (0007, 0008, 0017, 0018) still
        // cite the old capitalised name as if it were still in `AGENTS.md`. ADR 0016 already cites
        // the new location correctly, which is the evidence a reader needs to trust the register
        // moved rather than vanished. REVIEW #667 measured the counts fresh rather than trusting
        // the first draft's: `git grep -l 'Built And Not Wired' -- '*.md' '*.nix' '*.yml' '*.yaml'
        // '*.toml' '*.sh'` is 5 files (the four ADRs plus `nix/shipped.nix`'s own, still-true,
        // generic use - which is exactly why the wordings below are anchored to the ATTRIBUTION
        // and not the bare phrase) and `git grep -l 'Built and not wired' -- (same globs)` is 7
        // files / 8 hits. What excludes the six correct sites is the ATTRIBUTION, not the case: a
        // fourth wording below is the lowercase attribution, added because a plausible rewrite
        // using it is unmatched by the other three and no correct site names `AGENTS.md` this way.
        name: "AGENTS.md carries a Built And Not Wired section",
        wordings: &[
            "`AGENTS.md`'s *Built And Not Wired*",
            "AGENTS.md's *Built And Not Wired*",
            "AGENTS.md has a section named after that mistake",
            "`AGENTS.md`'s *Built and not wired*",
        ],
        evidence: &[Evidence {
            path: ".agents/skills/sutura/query-surface/SKILL.md",
            holds: "Built and not wired",
        }],
        instead: "the register is `.agents/skills/sutura/query-surface/SKILL.md`'s *Built and not \
                  wired* section now; `AGENTS.md` carries no such section. ADR 0016 cites it \
                  correctly",
        only: &[],
        // All four state the old name and amend it in place beside itself, per this repository's
        // own rule for a record. ADR 0016, which already cites the new location, is deliberately
        // NOT here - it has nothing to except.
        except: &[
            "docs/adr/0007-federating-across-different-data-systems.md",
            "docs/adr/0008-a-credential-per-leg-for-the-calling-subject.md",
            "docs/adr/0017-what-a-bigquery-test-runs-against.md",
            "docs/adr/0018-what-the-bigquery-wire-is-built-from.md",
        ],
    },
    Contradicted {
        // github.com/telekom/sutura#159. `CredentialBroker` shipped as a port
        // (`crates/sutura-domain/src/identity/credential.rs`) with a static implementor
        // (`sutura-config`), and ADR 0003's guarantee table still names it absent - the row this
        // record's own local-file path never needed, so nobody read it again after it went stale.
        name: "CredentialBroker does not exist",
        wordings: &["CredentialBroker` is still absent"],
        evidence: &[Evidence {
            path: "crates/sutura-domain/src/identity/credential.rs",
            holds: "pub trait CredentialBroker",
        }],
        instead: "`CredentialBroker` is a port in `sutura-domain`, with `sutura-config`'s \
                  `StaticCredentialBroker` as one implementor. A local file still has no login, so \
                  the guarantee's substance - nobody else to be, on this path - is unchanged; only \
                  the trait's existence is",
        only: &[],
        // 0003 states the old value in its guarantee table and amends it in place below the table.
        except: &["docs/adr/0003-datafusion-for-local-execution.md"],
    },
    Contradicted {
        // github.com/telekom/sutura#159. The conformance packs record's own *Consequences* section
        // asks for the gate this evidence names, which then shipped: `xtask/src/conformance.rs`
        // holds every registered data system to the packs or requires it declared unbound. Four
        // adapters (`duckdb`, `postgres`, the `datafusion` engine, `bigquery` -
        // `telekom/sutura#710`) bind `execute_packs!` today.
        name: "the conformance packs are unbuilt",
        wordings: &["accepted as the shape. None of it is built."],
        evidence: &[Evidence {
            path: "xtask/src/conformance.rs",
            holds: "Every registered data system is held to the conformance packs",
        }],
        instead: "`crates/sutura-conformance` and `execute_packs!` exist, four data systems bind \
                  them, and `xtask/src/conformance.rs` (`check-conformance-bindings`) holds the \
                  registration in step. What is still unbuilt: a per-pack timing aggregate and \
                  the `cargo-insta` unreferenced-snapshot check (both planned further down, not \
                  built), the three named corpus cases (remote-dimension filter with an orphan \
                  key, zero-denominator ratio, `CountDistinct` over two join keys), and the \
                  corpus itself (code today, not the files this record specifies). The fourth \
                  adapter, `sutura-exec-bigquery`, binds over a canned transport rather than a \
                  live endpoint - `crates/sutura-exec-bigquery/tests/conformance.rs`",
        only: &[],
        // 0012 states the old status line and amends it directly below.
        except: &["docs/adr/0012-conformance-packs-for-inputs-and-adapters.md"],
    },
    // The "four refusal reasons map to 422" row that used to be here (#603, #667) is deleted
    // rather than re-anchored, per its own last review note: it was corrected from four to five
    // by hand, #661 then added a sixth arm an hour fifty-eight minutes later, and every prose
    // site still said five (#676) - a third registration would be the same fix again. The number
    // is now read off the arms in `xtask/src/guidance/claims/counts.rs`'s `COUNTS`, which cannot
    // go stale the way a registered wording can, at the cost of needing digits rather than a
    // spelled number in prose.
    Contradicted {
        // github.com/telekom/sutura#159, raised in review of #667. `sutura serve` links the
        // adapter behind the default-off `bigquery` feature; a default build (feature off) still
        // links none of it and still refuses `kind: bigquery` by name, which is the half of the
        // sentence that survives. `sutura-serve` folded into `sutura-cli`'s `serve` module at
        // `github.com/telekom/sutura#685` step 2, so this is the same claim about the same code.
        name: "sutura serve links no BigQuery adapter",
        wordings: &["links no `BigQuery` adapter"],
        evidence: &[Evidence {
            path: "crates/sutura-cli/src/serve.rs",
            holds: "type BigQuerySource = sutura_exec_bigquery::BigQueryWarehouse",
        }],
        instead: "it links the adapter behind the default-off `bigquery` feature - \
                  `OpenedSources::BigQuery` and the `BigQuerySource` type alias. A default build \
                  (the feature off) still links none of it, which is the sense in which \
                  \"refuses `kind: bigquery` by name\" survives",
        only: &[],
        // 0018 states the old value and amends it directly below.
        except: &["docs/adr/0018-what-the-bigquery-wire-is-built-from.md"],
    },
    Contradicted {
        // github.com/telekom/sutura#159, raised in review of #714. Three sites in this ADR
        // understated the manifest and release side of the same crate: the artifact table's own
        // heading said no artifact links either half, the `sutura-serve` row said it is built by
        // no release package at all, and the paragraph below the table said the crate has no edge
        // and stays out of `[workspace.dependencies]`. All three are false the same way: the
        // `bigquery` feature is off by default, not absent from the manifests. `sutura-serve`
        // folded into `sutura-cli`'s `serve` module at `github.com/telekom/sutura#685` step 2, so
        // the row this claim is about is `nix/shipped.nix`'s one remaining `sutura` entry now.
        name: "no shipped artifact links either half of the BigQuery crate",
        wordings: &[
            "none of them link either half of this crate",
            "it is built by no release package at all",
            "declares no edge to `sutura-exec-bigquery`, and the root manifest keeps the crate out \
             of `[workspace.dependencies]`",
        ],
        evidence: &[
            Evidence {
                path: "nix/shipped.nix",
                holds: "probeFeatures = [ \"bigquery\" \"postgres\" ]",
            },
            Evidence {
                path: "crates/sutura-cli/Cargo.toml",
                holds: "dep:sutura-exec-bigquery",
            },
        ],
        instead: "`sutura-cli`'s manifest declares the `bigquery` feature and the edge, \
                  `nix/shipped.nix` packages it as a release artifact with `bigquery` in its \
                  `probeFeatures`, and the release workflow uploads its tarball. The root \
                  manifest's `[workspace.dependencies]` carries the crate too. What survives: no \
                  artifact links either half in its DEFAULT build, because the `bigquery` feature \
                  is off unless a build asks for it",
        only: &[],
        // 0018 states all three old values and amends each one in place directly below it.
        except: &["docs/adr/0018-what-the-bigquery-wire-is-built-from.md"],
    },
    Contradicted {
        // #792: found spot-checking stale `file.rs:NNN` citations. `DefinitionDigest::of` gained
        // a third parameter when #181 composed N metadata sources into one bundle; ADR-0007's
        // digest correction still claimed the pre-#181 arity, which made a correct line number
        // sit under a now-wrong sentence.
        //
        // #794 registered the wording above from ADR-0006/0007. ADR-0009 carried the same false
        // arity in a third phrasing - `&Definitions` and `&Knowledge`, no definite articles - which
        // the exact-substring ratchet missed. A SECOND literal entry, not a shortened shared one:
        // "and nothing else" alone appears in over a hundred unrelated legitimate lines across
        // `docs/`, so widening to that substring would forbid sentences nobody meant to forbid.
        name: "DefinitionDigest::of takes only Definitions and Knowledge",
        wordings: &[
            "takes the `Definitions` and the `Knowledge` and nothing else",
            "takes `&Definitions` and `&Knowledge` and nothing else",
        ],
        evidence: &[Evidence {
            path: "crates/sutura-domain/src/definitions.rs",
            holds: "manifest: &ContributionManifest,",
        }],
        instead: "`DefinitionDigest::of` takes the `Definitions`, the `Knowledge` and a \
                  `ContributionManifest`, and nothing else. The paragraph's real point survives \
                  unchanged: `QueryPlan` and `LegPlan` are in neither, so no plan-shape change \
                  moves the digest",
        only: &[],
        except: &[],
    },
    Contradicted {
        // #792: found the same way. `permitted_for` traded its `&Request` parameter for the
        // `&Asked` `establish_asked` already derived, and gained `run_sql_enabled`, when #666
        // added the off-by-default raw SQL tool - after ADR-0023 quoted the older signature.
        name: "permitted_for takes a Request and returns Permitted alone",
        wordings: &["pub fn permitted_for(request: &Request) -> Permitted"],
        evidence: &[Evidence {
            path: "crates/sutura-http/src/capability.rs",
            holds: "pub fn permitted_for(asked: &Asked, run_sql_enabled: bool) -> Permitted",
        }],
        instead: "`crates/sutura-http/src/capability.rs` declares \
                  `pub fn permitted_for(asked: &Asked, run_sql_enabled: bool) -> Permitted`: it \
                  takes the `Asked` `establish_asked` already derived, not the raw request, and \
                  narrows by a deployment-level switch afterward",
        only: &[],
        except: &[],
    },
    Contradicted {
        // #795 item 2. `#732` (`0ea25539`, `#378` PR2) gave `call_tool` a `context` parameter it
        // actually reads through `AgentSurface::asked` - ADR-0023's own paragraph, written before
        // that PR, still describes the parameter as bound to `_context` and discarded.
        name: "the agent surface discards its per-request context",
        wordings: &["already takes that value and discards it"],
        evidence: &[Evidence {
            path: "crates/sutura-mcp/src/server.rs",
            holds: "let asked = self.asked(&context)?;",
        }],
        instead: "`crates/sutura-mcp/src/server.rs`'s `call_tool` names the parameter `context`, \
                  not `_context`, and reads it through `AgentSurface::asked`; \
                  `Asking::PerRequest` pulls a caller out of `context.extensions`. \
                  `docs/adr/0023`'s `Amendment, 2026-09-16` carries the correction",
        only: &[],
        except: &["docs/adr/0023-how-the-agent-surface-learns-who-is-asking.md"],
    },
    Contradicted {
        // #795 item 3. `feat/source-registry` (`crates/sutura-config/src/sources.rs`) landed the
        // per-source adapter selection that ADR-0007's `feat/source-registry` bullet still cites
        // `docs/architecture.md:595-600` for as deliberately absent - and the range that
        // citation names has since moved to a different topic (push-down and federation) too.
        name: "the source-to-adapter selection is deliberately absent",
        wordings: &["the source-to-adapter selection `docs/architecture.md:595-600` records as deliberately absent"],
        evidence: &[Evidence {
            path: "crates/sutura-config/src/sources.rs",
            holds: "pub enum SourceKind",
        }],
        instead: "`crates/sutura-config/src/sources.rs` parses a `sources:` tree per deployment: a \
                  `SourceName` selects a warehouse out of a registry rather than being compared \
                  for equality against the one linked adapter. `docs/architecture.md#what-exists-\
                  today` and `docs/adr/0007`'s `Amendment, 2026-09-16` carry the correction",
        only: &[],
        except: &["docs/adr/0007-federating-across-different-data-systems.md"],
    },
    Contradicted {
        // `github.com/telekom/sutura#800` retired these and `github.com/telekom/sutura#802`
        // ratchets them. Both presented leg 2 as an existing, selectable mode, which is the shape
        // this table is for: the README text was true of the design and false of the build, and a
        // correction in place alone would let whoever reads the older commit reinstate it.
        name: "a multiplayer mode exists and is selectable per connection",
        // TWO sentences, false for the same reason and registered separately. The second is the
        // reselectable half of the first: even granting a mode existed, nothing carries the
        // credential that would make choosing it meaningful.
        //
        // The ratchet matches an EXACT substring, not a paraphrase: a reworded version of either
        // sentence would pass and nothing fires. That is deliberate - the phrase fragments here
        // are ordinary English that appears in many correct sentences, so a shortened or shared
        // substring would refuse text nobody meant to forbid.
        wordings: &[
            "There are 2 flavours of sutura:",
            "Per connection the mode can be configured.",
        ],
        // The sentence that refutes it, stated today where the true limit is set out. It is the
        // anchor rather than prose recalled: `docs/serving.md` says no source executes as the
        // asking subject, so there is nothing to select.
        evidence: &[Evidence {
            path: "docs/serving.md",
            holds: "no adapter in this build can carry a per-subject credential",
        }],
        instead: "single player is what ships and the only mode any connection runs; multiplayer \
                  is the design target. No adapter in this build can carry a per-subject \
                  credential, so no source executes as the asking subject and nothing selects it",
        only: &[],
        except: &[],
    },
    Contradicted {
        // `github.com/telekom/sutura#892` shipped the push from the agent surface, which falsified
        // deviation 7's own wording in three files at once. The wording registered here is the one
        // that was WRITTEN (`docs/adr/0015-...md:330`) rather than a paraphrase, because a rule
        // against a sentence nobody wrote is what
        // `a_page_a_rule_exempts_holds_a_wording_that_rule_forbids` refuses.
        name: "the spend headroom gauge is pushed from one transport only",
        wordings: &["pushed from one transport only", "pushed from a single transport"],
        evidence: &[
            Evidence {
                path: "crates/sutura-cli/src/serve/agent.rs",
                holds: "push_headroom",
            },
            // The handoff stopped being the composition root's habit: `AgentMount::new` cannot be
            // called without a `SpendHeadroomPush`, whose gauge-carrying variant only
            // `SpendHeadroomPush::of(&state)` can build.
            Evidence {
                path: "crates/sutura-http/src/state.rs",
                holds: "pub enum SpendHeadroomPush",
            },
        ],
        instead: "the composition root hands this state's own gauge into the `Serving` wrapper, \
                  which pushes after both `Surface::answer` and `Surface::run_sql`, so the agent \
                  transport and `POST /v1/query` drive one series - with the asymmetry the record \
                  states, that `POST /v1/run_sql` pushes nothing - and the handoff is held by a \
                  type rather than by that root, since `AgentMount::new` requires a \
                  `SpendHeadroomPush` whose gauge-carrying variant only \
                  `SpendHeadroomPush::of(&state)` can build",
        only: &[],
        // The Second amendment's deviation 7 is quoted in order to be corrected in place, so this
        // record holds the one copy of the wording no scan may refuse.
        except: &["docs/adr/0015-an-authenticated-metrics-endpoint.md"],
    },
];

/// `CONTRADICTED` rows a later commit deleted rather than re-anchored, kept only for their
/// `wordings` ratchet. See [`super::Withdrawn`] for what that means and does not mean.
pub(in crate::guidance) const WITHDRAWN: &[Withdrawn] = &[Withdrawn {
    // #606 added this row, keyed on the connector rather than the words: ADR 0025 keeps the
    // superseded sentence, in italics and marked wrong, quoting it a second time to correct
    // it - so a rule on the bare sentence would refuse the record for quoting what it
    // corrects. `because` is the assertion; the ADR's own quote at `:409-410` drops it, which
    // is why this wording does not fire there. #770 deleted the row itself rather than
    // re-anchor `Evidence` on "3016 sysroot files": that count came from one external CodeQL
    // run with no live counterpart in this tree to recompute it from - unlike `counts.rs`'s
    // `Counted` rows, which all read a literal out of code that still exists.
    name: "a buildless CodeQL database leaves out `alloc`/`std`",
    issue: "#603, #770",
    wordings: &["because a buildless database extracts the crate's own dependencies but not"],
    except: &[],
}];
