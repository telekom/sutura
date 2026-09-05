//! The checks that judge a CLAIM rather than a line: `claims` here, `counts` beside it.
//!
//! Split out of `guidance.rs` mechanically - see the `mod claims` comment there - and nothing
//! moved here was changed on the way. What makes the seam a real one rather than a line count:
//! the checks that stayed judge one LINE at a time, and these cannot. Prose wraps, so a claim
//! is found in a flattened view of a file and a count is derived by walking the tree. [`flatten`]
//! is that view, and it is the only thing the two share.
//!
//! `claims` and `counts` exist because of one review, and because of one CAUSE rather than
//! nineteen mistakes: nineteen false sentences across `README.md`, `docs/` and `AGENTS.md`, four of
//! them saying there is no HTTP surface while two crates and a published page ship one, and in
//! almost every case the corrected sentence already existed in a sibling document. The correction
//! had landed in one file and had not been carried to the others. So what is checked here is the
//! CLAIM rather than the file: one [`Contradicted`] entry carries every wording of one claim, which
//! is what makes a sibling that was missed a failure rather than a survivor.
//!
//! **What these two cannot do, stated before a reader trusts them.** They match a literal, so a
//! paraphrase escapes - the same limit `AGENTS.md` records for the leak guard. They are a ratchet
//! on a sentence somebody has already written once, not a reader.

use std::path::Path;

// Both split off under the 1000-line cap, on the seam the causality gate forces: the mechanism
// moves and every assertion stays in `tests` below, in the file that DECLARES the module - a file
// adding no `#[test]` is one that gate may revert, which would take the declaration with it and
// orphan the tests. `remedies` judges the sentence a claim hands a reader; `counts` is the whole
// second check named in the header above, and it went rather than the table because
// [`CONTRADICTED`] is what grows by ENTRY - one claim is about twenty lines - so this is the file
// that has to have room.
//
// **`counts` is a CHILD rather than a sibling of `claims`, and that is deliberate.** `guidance.rs`
// lists the two as peers, which argues for a sibling; against that, `flatten` below is the one
// thing they share, and a child reads it while private. A peer would need it exported to all of
// `guidance` to borrow it, which is a wider change than the one the placement buys.
mod counts;
mod remedies;

pub(super) use counts::{COUNTS, count_mismatches};
pub(super) use remedies::remedy_problems;

/// What makes a claim false: a path that is there, optionally holding a literal.
///
/// A path rather than a sentence, because the point of this table is that the prose is checked
/// against the tree and not against a second piece of prose.
pub(super) struct Evidence {
    /// Repo-relative path.
    path: &'static str,
    /// A literal the file must hold. Empty means the path existing is the whole evidence.
    holds: &'static str,
}

impl Evidence {
    /// Is the evidence there?
    fn stands(&self, root: &Path) -> bool {
        let full = root.join(self.path);
        if self.holds.is_empty() {
            return full.exists();
        }
        std::fs::read_to_string(&full).is_ok_and(|text| text.contains(self.holds))
    }
}

/// A claim the tree contradicts.
///
/// [`Pin`](super::Pin) above holds a VERSION against its source; this holds a STATEMENT against its source, and
/// the difference that matters is the evidence. A rule is live only while every `Evidence` stands,
/// so a rule about a crate that is later deleted retires itself rather than forbidding a sentence
/// that has become true again - which a `Forbidden` entry cannot do, being unconditional.
pub(super) struct Contradicted {
    /// Human name, for the message.
    name: &'static str,
    /// Every wording of the same claim, whitespace-collapsed. One entry, N sibling documents: a
    /// wording is added here the moment it is found, not merely fixed where it was found.
    wordings: &'static [&'static str],
    /// What refutes it. ALL of these must stand for the rule to be live.
    ///
    /// Doubly load-bearing, and the second job is why `github.com/telekom/sutura#241` was filed:
    /// the same rows underwrite [`Contradicted::instead`]. A remedy that rests on a row here
    /// retires with the rule instead of outliving it, and
    /// `tests::every_live_rule_still_has_its_evidence` is where a row that has gone reports itself.
    evidence: &'static [Evidence],
    /// What is true instead, and where the correct sentence already is.
    ///
    /// **Held by [`remedies`], and not by review** - which it was, for as long as it took a person
    /// to read this field: it said the agent surface was absent from the day `sutura mcp` shipped,
    /// invisible because `run`'s scope is prose files and this gate never reads its own source.
    ///
    /// **What that does not reach is the sentence.** A remedy is prose and nothing derives it, so a
    /// wording nobody has registered, a number, or a claim about behaviour is held here by review -
    /// the same limit `AGENTS.md` records for this whole module. What is closed is the class that
    /// rotted: a remedy asserting an absence the tree contradicts, once that absence is a wording.
    instead: &'static str,
    /// Paths this applies to. Empty means everywhere in scope.
    only: &'static [&'static str],
    /// Paths exempt - typically a page that quotes the wrong sentence in order to correct it.
    except: &'static [&'static str],
}

impl Contradicted {
    /// Is what refutes this claim still in the tree?
    ///
    /// A rule that kept forbidding a sentence after it became true again is the same bug as a
    /// stale doc, one layer up. Read by both checks over this table, so *live* means one thing.
    fn is_live(&self, root: &Path) -> bool {
        self.evidence.iter().all(|e| e.stands(root))
    }
}

pub(super) const CONTRADICTED: &[Contradicted] = &[
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
        name: "five refusal codes are 422",
        wordings: &["five of the codes above land", "five of the eleven refusal codes"],
        evidence: &[Evidence {
            path: "crates/sutura-http/src/wire/refusal.rs",
            holds: "four variants are `422`",
        }],
        instead: "four: `grain_not_supported`, `time_range_too_long`, `too_many_dimensions` and \
                  `duplicate_dimension`. `docs/serving.md`'s own table two lines above the sentence \
                  lists four, which is the tell that the number was carried and the table was not",
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
];

/// A file's text with every run of whitespace collapsed to one space, plus the line each byte
/// came from.
///
/// **The reason this is not line-based.** Three of the false sentences this gate now holds are
/// WRAPPED in their source file: `no logging\ndependency at all` and `the only data\nsystem
/// adapter is DuckDB` are both invisible to a line-by-line search, and a check that missed the
/// exact copies it was written for would have been worse than no check. Prose wraps; a claim does
/// not. `stale_phrases` above stays line-based on purpose - its needles are command lines, where a
/// newline is a real difference.
///
/// **The limit, because it cost a wording in the table above.** A comment marker is not stripped, so
/// a claim wrapping inside a `#` comment block flattens with the `#` mid-sentence and is not found.
/// Stripping markers was rejected: `#` also starts a markdown heading, and joining a heading to the
/// paragraph before it could match a "claim" spanning two sections. A claim written across two
/// comment lines needs a needle that fits on one of them.
pub(in crate::guidance) fn flatten(text: &str) -> (String, Vec<usize>) {
    let mut flat = String::with_capacity(text.len());
    let mut lines = Vec::with_capacity(text.len());
    let mut line = 1_usize;
    let mut pending = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if character == '\n' {
                line = line.saturating_add(1);
            }
            pending = !flat.is_empty();
            continue;
        }
        if pending {
            flat.push(' ');
            // The separator is attributed to the line the NEXT word is on, so a wrapped match is
            // reported where a reader would start reading it.
            lines.push(line);
            pending = false;
        }
        let before = flat.len();
        flat.push(character);
        for _ in before..flat.len() {
            lines.push(line);
        }
    }
    (flat, lines)
}

/// Every line, one-based, on which `wording` appears in the flattened `text`.
fn wording_lines(text: &str, wording: &str) -> Vec<usize> {
    let (flat, lines) = flatten(text);
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(at) = flat.get(from..).and_then(|rest| rest.find(wording)) {
        let offset = from.saturating_add(at);
        found.push(lines.get(offset).copied().unwrap_or(1));
        from = offset.saturating_add(wording.len().max(1));
    }
    found
}

pub(super) fn contradicted_claims(root: &Path, files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for rule in CONTRADICTED {
        if !rule.is_live(root) {
            continue;
        }
        for rel in files {
            if !rule.only.is_empty() && !crate::repo::matches_any(rule.only, rel) {
                continue;
            }
            if crate::repo::matches_any(rule.except, rel) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
                continue;
            };
            for wording in rule.wordings {
                for line in wording_lines(&text, wording) {
                    problems.push(format!(
                        "{rel}:{line}: \"{wording}\" - {} is not true of this repo\n      \
                         what is true: {}",
                        rule.name, rule.instead
                    ));
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::remedies::{path_shaped, remedies_hold, remedy_scan_broke};
    use super::{CONTRADICTED, COUNTS, Contradicted, Evidence, remedy_problems};

    /// The entry as it stood before `github.com/telekom/sutura#241`, reduced to the two fields
    /// that made it wrong: evidence that the surface SHIPS, and a remedy saying it does not.
    const STALE: Contradicted = Contradicted {
        name: "a transport surface is absent",
        wordings: &["the MCP surface is absent"],
        evidence: &[Evidence {
            path: "crates/sutura-mcp/Cargo.toml",
            holds: "",
        }],
        instead: "the MCP surface is absent and the HTTP one is not: `sutura-http` and \
                  `sutura-serve` ship",
        only: &[],
        except: &[],
    };

    #[test]
    fn an_instead_sentence_that_names_an_absent_surface_fails_when_the_surface_lands() {
        let root = crate::repo::root().expect("the repo root");
        // The surface landed: the evidence row stands, so the rule is live and its remedy is read.
        assert!(STALE.is_live(&root), "the fixture's premise is that the crate is there");
        let found = remedies_hold(&root, &[&STALE]);
        assert!(
            found.iter().any(|problem| problem.contains("the MCP surface is absent")),
            "a remedy repeating a wording this table forbids must be reported: {found:?}"
        );
    }

    /// Four citations: two that resolve - a numbered record by its PREFIX, which is how this repo
    /// cites one, and a recipe - and two that do not.
    const CITES: Contradicted = Contradicted {
        name: "citations",
        wordings: &["a phrase nothing in this repo writes"],
        evidence: &[],
        instead: "see `docs/adr/0002` and `just mcp-e2e`, not `docs/no-such-page.md` or \
                  `just no-such-recipe`",
        only: &[],
        except: &[],
    };

    #[test]
    fn a_remedy_citation_that_resolves_to_nothing_is_reported() {
        let root = crate::repo::root().expect("the repo root");
        let found = remedies_hold(&root, &[&CITES]);
        assert!(
            found.iter().any(|problem| problem.contains("docs/no-such-page.md")),
            "a path that is not in the tree must be reported: {found:?}"
        );
        assert!(
            found.iter().any(|problem| problem.contains("no-such-recipe")),
            "a recipe that does not exist must be reported: {found:?}"
        );
        assert_eq!(found.len(), 2, "the two that resolve must not be reported: {found:?}");
    }

    #[test]
    fn a_settings_key_or_a_route_is_not_a_citation() {
        // The three spans in the live table that hold a slash or look like they might, and none of
        // them is something to open. Under-claiming is the safe direction here.
        assert!(!path_shaped("sources.<alias>.kind"));
        assert!(!path_shaped("GET /v1/catalog"));
        assert!(!path_shaped("ci.yml"));
        assert!(path_shaped("docs/serving.md"));
        assert!(path_shaped(".agents/skills/sutura/query-surface/SKILL.md"));
    }

    #[test]
    fn a_remedy_scan_that_reads_no_path_reports_itself() {
        // Same argument as `a_count_entry_measures_something`: a span walk that stopped working
        // would make the check pass on every remedy. Nothing to read is the tell.
        assert!(!remedy_scan_broke(&[]).is_empty(), "an empty table must not read as clean");
        let live: Vec<&Contradicted> = CONTRADICTED.iter().collect();
        assert!(remedy_scan_broke(&live).is_empty(), "the live table cites paths");
    }

    #[test]
    fn every_live_remedy_is_held_to_the_prose_it_corrects() {
        // The live half of the two fixtures above. Red against the tree this was written on: the
        // transport entry's remedy said the agent surface was absent while its own evidence rows
        // prove it ships.
        let found = remedy_problems(&crate::repo::root().expect("the repo root"));
        assert!(found.is_empty(), "a remedy in CONTRADICTED does not hold: {found:?}");
    }

    #[test]
    fn a_wrapped_sentence_is_still_one_claim() {
        use super::wording_lines;
        // THE case this check exists for: two sentences it was written to hold wrap in their file,
        // so a line-based search finds neither. Reported on the line a reader starts them on.
        let wrapped = "There is no audit sink in the workspace and no logging\ndependency at all.";
        assert_eq!(wording_lines(wrapped, "no logging dependency at all"), vec![1]);

        let later = "line one\nline two\nbroker, no audit sink, and the only data\nsystem adapter is DuckDB.";
        assert_eq!(wording_lines(later, "the only data system adapter is DuckDB"), vec![3]);
    }

    #[test]
    fn a_wording_that_is_absent_is_not_reported() {
        use super::wording_lines;
        assert!(
            wording_lines(
                "There IS an HTTP surface, and its token authenticates the deployment.",
                "no MCP server and no HTTP surface"
            )
            .is_empty()
        );
        // Every occurrence, not just the first: two siblings in one file is the shape this
        // whole table exists for.
        let twice = "no logging dependency at all\nand again, no logging dependency at all\n";
        assert_eq!(wording_lines(twice, "no logging dependency at all"), vec![1, 2]);
    }

    #[test]
    fn tabs_and_runs_of_spaces_collapse_the_same_way_a_newline_does() {
        use super::wording_lines;
        // A markdown table cell pads with spaces and a code block indents with tabs. Neither is
        // a difference in the claim, and treating either as one would leave a hole in a file
        // format this repo writes most of its prose in.
        assert_eq!(wording_lines("| x |  four   exact\tsets | y |", "four exact sets"), vec![1]);
    }

    #[test]
    fn evidence_is_the_path_and_not_a_second_opinion() {
        let root = crate::repo::root().expect("the repo root");
        // Existence alone.
        assert!(
            Evidence {
                path: "AGENTS.md",
                holds: ""
            }
            .stands(&root)
        );
        // The absent side of the same check. **Not the manifest of a PLANNED crate**, which is what
        // this used to be: `crates/sutura-mcp/Cargo.toml` was the fixture until the agent surface
        // landed, and then a test about path existence started failing because a crate got written.
        // A path with a name nothing will ever take cannot go the same way.
        assert!(
            !Evidence {
                path: "crates/no-such-crate-exists/Cargo.toml",
                holds: ""
            }
            .stands(&root)
        );
        // And a literal inside it.
        assert!(
            Evidence {
                path: "AGENTS.md",
                holds: "identity-aware semantic data runtime"
            }
            .stands(&root)
        );
        assert!(
            !Evidence {
                path: "AGENTS.md",
                holds: "a phrase nothing in this repo writes"
            }
            .stands(&root)
        );
    }

    #[test]
    fn every_live_rule_still_has_its_evidence() {
        // A rule whose evidence has gone is SILENT - right behaviour, and a failure mode nobody
        // notices. This is the tell: either the claim became true, in which case delete the row
        // the way the invariants table says, or its anchor was renamed and needs replacing.
        let root = crate::repo::root().expect("the repo root");
        for rule in CONTRADICTED {
            for evidence in rule.evidence {
                assert!(
                    evidence.stands(&root),
                    "`{}` no longer has its evidence: {} (holds `{}`)",
                    rule.name,
                    evidence.path,
                    evidence.holds
                );
            }
            assert!(!rule.wordings.is_empty(), "`{}` forbids nothing", rule.name);
            // An empty list is vacuously true, so an entry with no evidence can never retire and
            // the loop above would check nothing about it.
            assert!(!rule.evidence.is_empty(), "`{}` rests on nothing", rule.name);
        }
    }

    #[test]
    fn a_page_a_rule_exempts_holds_a_wording_that_rule_forbids() {
        // `except` is for the record that quotes the wrong sentence in order to CORRECT it - and it
        // is therefore also the one copy of that sentence in the tree no scan reads. Those two facts
        // together are how an exemption outlives its reason: the correction gets reworded, the quote
        // goes, and what is left is a rule registered against a sentence nobody wrote plus a page
        // permanently exempt from it. Neither half reports itself, so the exemption is made to keep
        // earning itself here.
        //
        // Over the glob rather than the literal, because `except` is matched as one - a directory
        // pattern that no longer covers a page that quotes the sentence is the same stale exemption
        // in a shape a path read would have missed.
        //
        // `is_live` for the same reason the gate reads it: a retired rule forbids nothing, so
        // demanding the page keep quoting it would turn tidying that quote red for a rule that is
        // no longer there. Both predicates, or this test and the check disagree about what a rule is.
        let root = crate::repo::root().expect("the repo root");
        let crate::repo::RepoFiles { files, .. } = crate::repo::all_files().expect("could not list the repo");
        for rule in CONTRADICTED
            .iter()
            .filter(|rule| !rule.except.is_empty() && rule.is_live(&root))
        {
            let quoted = files
                .iter()
                .filter(|rel| crate::repo::matches_any(rule.except, rel))
                .filter_map(|rel| std::fs::read_to_string(root.join(rel)).ok())
                .any(|text| {
                    rule.wordings
                        .iter()
                        .any(|wording| !super::wording_lines(&text, wording).is_empty())
                });
            assert!(
                quoted,
                "`{}` exempts {:?}, and nothing there states any wording it forbids - so the \
                 exemption protects nothing. Delete it, or delete the rule",
                rule.name, rule.except
            );
        }
    }

    #[test]
    fn the_number_read_is_the_one_immediately_before_the_marker() {
        use super::counts::stated_numbers;
        // The real sentence, and the real trap in it: the line carries `10001` as well, so a
        // check that read every number on it would report the row cap as a golden count.
        let line = "Both legs pinned: 63 SQL goldens read `LIMIT 10001`, and the engine leg asserts the fetch.";
        assert_eq!(stated_numbers(line, "SQL goldens read"), vec![(1, 63)]);
        // No number there is not a claim about the count.
        assert_eq!(
            stated_numbers("The SQL goldens read the row cap.", "SQL goldens read"),
            vec![]
        );
        // Nor is a line that does not carry the marker at all.
        assert_eq!(stated_numbers("39 of something else entirely", "SQL goldens read"), vec![]);
        // The page that EXPLAINS the marker states no number and must stay unread - this is the
        // sentence a "every page naming the marker carries a number" rule would have failed.
        assert_eq!(
            stated_numbers(
                "the number written before the marker `SQL goldens read` and compares",
                "SQL goldens read"
            ),
            vec![]
        );
    }

    #[test]
    fn a_statement_that_wraps_away_from_its_marker_is_still_read() {
        use super::counts::stated_numbers;
        // THE hole this replaced. Both of these were invisible to the per-line reader, and both
        // are indistinguishable from agreement in its output: the first because the number ended
        // one line above the marker, the second because only the first marker on a line was read.
        // A reflow of a paragraph is enough to produce the first, which is why a sentence saying
        // "keep them on one line" was not the fix.
        let wrapped = "the whole set is 10
SQL goldens read the cap";
        assert_eq!(stated_numbers(wrapped, "SQL goldens read"), vec![(2, 10)]);
        let twice = "| 93 SQL goldens read here | 94 SQL goldens read there |";
        assert_eq!(stated_numbers(twice, "SQL goldens read"), vec![(1, 93), (1, 94)]);
    }

    #[test]
    fn a_count_entry_measures_something() {
        // Same argument as `every_live_rule_still_has_its_evidence`, for the other table: a glob
        // matching nothing would make the check pass vacuously. Caught here, not on a branch.
        let root = crate::repo::root().expect("the repo root");
        let crate::repo::RepoFiles { files, .. } = crate::repo::all_files().expect("could not list the repo");
        for counted in COUNTS {
            assert!(
                super::counts::tally(&root, &files, counted) > 0,
                "the {} count matches nothing - `{}` under {:?}",
                counted.name,
                counted.holds,
                counted.over
            );
        }
    }

    #[test]
    fn a_count_entry_is_compared_against_a_page_that_states_it() {
        // The mirror of the test above, and it is here because the failure it describes HAPPENED:
        // the row-cap sentence lived in `AGENTS.md`, the router rewrite carried the invariants
        // table into `.agents/skills/`, `mentioned_in` stayed as it was, and the gate went on
        // counting 93 goldens against a number no page stated any more. A count nobody writes
        // down is not a gate - it is a walk of the tree whose verdict is always agreement.
        let root = crate::repo::root().expect("the repo root");
        let crate::repo::RepoFiles { files, .. } = crate::repo::all_files().expect("could not list the repo");
        for counted in COUNTS {
            assert!(
                !super::counts::statements(&root, &files, counted).is_empty(),
                "no page under {:?} states the {} count before `{}`",
                counted.mentioned_in,
                counted.name,
                counted.marker
            );
        }
    }

    #[test]
    fn a_second_occurrence_in_one_file_counts_twice_only_where_the_entry_says_so() {
        use super::counts::Granularity;
        // THE capability this table lacked, and the reason it could not hold the `pub trait`
        // count: two declarations in one file are two ports and one file. Both entries are right
        // about their own literal, and neither answer is a safe default for the other - a golden
        // carrying the row cap twice is still one golden.
        let twice = "pub trait One {}\npub trait Two {}\n";
        assert_eq!(Granularity::Files.count_in(twice, "pub trait "), 1);
        assert_eq!(Granularity::Occurrences.count_in(twice, "pub trait "), 2);
        // Absent counts zero either way, which is what the tally check above reads.
        assert_eq!(Granularity::Files.count_in("mod tests {}\n", "pub trait "), 0);
        assert_eq!(Granularity::Occurrences.count_in("mod tests {}\n", "pub trait "), 0);
        // Non-overlapping, like `grep -o`: `aa` in `aaaaa` is two, not the four an overlapping
        // scan would report.
        assert_eq!(Granularity::Occurrences.count_in("aaaaa", "aa"), 2);
    }
}
