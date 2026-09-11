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

use super::{Contradicted, Evidence};

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
            Evidence {
                path: "crates/sutura-serve/Cargo.toml",
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
        instead: "both surfaces ship. `sutura-http` and `sutura-serve` serve HTTP and \
                  `sutura-mcp` serves the agent surface over the process's own pipes, `just \
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
        // ANCHORED ON `because`, and that is the whole design of this row: the record KEEPS the
        // superseded sentence, in italics and marked wrong, seventy lines above the measurement
        // that replaced it - and then quotes it a second time inside that correction. A rule
        // keyed on the sentence alone would refuse a record for quoting what it corrects, so what
        // is forbidden is ASSERTING it, which is the connector rather than the words.
        name: "a buildless CodeQL database leaves out `alloc`/`std`",
        // Three wordings for the TWO sentences `github.com/telekom/sutura#538` names: the `because`
        // line is one literal of the first, and neither of the other two exists anywhere in the
        // tree - so the row is a ratchet on a re-assertion rather than a refusal of prose already
        // written. They need no `because` anchor because the record does not quote either of them;
        // it quotes only the "extracts ... but not" phrasing.
        wordings: &[
            "because a buildless database extracts the crate's own dependencies but not",
            "does not extract `alloc`/`std`",
            "standard library is absent",
        ],
        // The measurement that replaced the cause. If it ever leaves the record, this rule
        // retires rather than forbidding a sentence nothing in the tree disproves any more.
        evidence: &[Evidence {
            path: "docs/adr/0025-what-a-scorecard-zero-says-about-this-repository.md",
            holds: "3016 sysroot files",
        }],
        instead: "the sysroot IS extracted - \
                  `docs/adr/0025-what-a-scorecard-zero-says-about-this-repository.md` counts 3016 \
                  files and 98903 functions from it - and no `alloc::`, `core::` or `std::` \
                  canonical path exists in the database at all, because the extractor could not \
                  determine the edition and left those files UNRESOLVED. Same consequence, \
                  different mechanism, and the difference is what costs: turning \
                  dependency-as-source extraction on does not reach an unresolved file",
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
        // The wording that would be FALSE today: no binary in this repository can open
        // `catalog.kind: datahub` (the crate is a `sutura-app` dev-dependency and `sutura-serve`
        // refuses the kind by name), so a README or record that presents the DataHub deployment
        // shape as a SERVED deployment contradicts the tree. The entry is a RATCHET rather than a
        // repair: it forbids the availability wording wherever it appears, and it deliberately does
        // NOT forbid the shape wording, which is the honest way to describe a deployment nothing
        // can open. `docs/adr/0016` and the *Built and not wired* register carry the limit itself.
        wordings: &[
            "a served deployment with per-caller identities",
            "a served deployment whose semantic catalog is DataHub",
        ],
        evidence: &[Evidence {
            path: "crates/sutura-serve/src/catalog.rs",
            holds: "CatalogKind::Datahub =>",
        }],
        instead: "write the deployment SHAPE, whose runnable proof is test code over a recorded \
                  fixture: no binary links the adapter and `sutura-serve` refuses the kind by name, \
                  which the *Built and not wired* register in \
                  `.agents/skills/sutura/query-surface/SKILL.md` records",
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
                path: "crates/sutura-serve/src/main.rs",
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
        wordings: &["scoped view"],
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
];
