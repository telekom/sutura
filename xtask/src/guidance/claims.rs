//! The two checks that judge a CLAIM rather than a line: `claims` and `counts`.
//!
//! Split out of `guidance.rs` mechanically - see the `mod claims` comment there - and nothing
//! moved here was changed on the way. What makes the seam a real one rather than a line count:
//! the checks that stayed judge one LINE at a time, and these two cannot. Prose wraps, so a claim
//! is found in a flattened view of a file and a count is derived by walking the tree.
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

use super::matches_any;

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
/// [`Pin`] above holds a VERSION against its source; this holds a STATEMENT against its source, and
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
    evidence: &'static [Evidence],
    /// What is true instead, and where the correct sentence already is.
    instead: &'static str,
    /// Paths this applies to. Empty means everywhere in scope.
    only: &'static [&'static str],
    /// Paths exempt - typically a page that quotes the wrong sentence in order to correct it.
    except: &'static [&'static str],
}

pub(super) const CONTRADICTED: &[Contradicted] = &[
    Contradicted {
        name: "there is no HTTP surface",
        wordings: &[
            "neither an MCP nor an HTTP surface",
            "The MCP and HTTP surfaces",
            "no MCP server and no HTTP surface",
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
        ],
        instead: "the MCP surface is absent and the HTTP one is not: `sutura-http` and \
                  `sutura-serve` ship, `docs/serving.md` is published, and `examples/single-player` \
                  has a captured session. The token authenticates the DEPLOYMENT, not the caller, \
                  which is what keeps the identity claims design targets - see `docs/concepts.md`",
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
];

/// What one file contributes to a count: itself, or every occurrence in it.
///
/// A required field on [`Counted`] rather than a default, because the wrong answer is SILENTLY
/// green. This used to be a sentence in `holds`'s documentation - *"files, not occurrences"* - and
/// a sentence is held by recall: the row-cap entry wants files and says why, and the `pub trait`
/// entry below is right at file granularity only for as long as no two declarations share a file.
/// A closed enum matched exhaustively is what makes the next entry answer the question instead of
/// inheriting an answer.
#[derive(Clone, Copy)]
pub(super) enum Granularity {
    /// One per file, however often the literal appears in it.
    Files,
    /// Every occurrence, non-overlapping - what `grep -o` counts.
    Occurrences,
}

impl Granularity {
    /// What one file's `text` contributes to the total.
    fn count_in(self, text: &str, holds: &str) -> u64 {
        match self {
            Self::Files => u64::from(text.contains(holds)),
            // Saturating rather than `as`: a `usize` count cannot exceed `u64::MAX` on any target
            // this builds for, and the cast lint is on for the case where that reasoning is wrong.
            Self::Occurrences => u64::try_from(text.matches(holds).count()).unwrap_or(u64::MAX),
        }
    }
}

/// A number in prose that counts something in the tree.
///
/// The third shape of the same idea. [`Pin`] reads its value from a line in a file; this one
/// DERIVES it by counting, which is the only honest way to hold a number nobody is going to
/// recount by hand. `39 SQL goldens read LIMIT 10001` was written when there were 39 and stayed
/// written when there were 63, and no reviewer is going to notice that twice.
pub(super) struct Counted {
    /// Human name, for the message.
    name: &'static str,
    /// Files to count over. Globs against repo-relative paths.
    over: &'static [&'static str],
    /// The literal that is counted. Whether a second one in the same file counts is
    /// [`Counted::granularity`]'s question, not this field's.
    holds: &'static str,
    /// Files, or occurrences within them.
    granularity: Granularity,
    /// Where the number may be stated. **At least one file here has to state it**, or the entry
    /// counts the tree and compares it to nothing - see [`count_mismatches`].
    mentioned_in: &'static [&'static str],
    /// The noun phrase the number belongs to. The count is the integer IMMEDIATELY BEFORE it.
    ///
    /// Deliberately that narrow, for the reason [`contradicts`] is narrow: the sentence carrying
    /// `39 SQL goldens read LIMIT 10001` also carries `10001`, and a check that read every number
    /// on the line would report the row cap as a wrong golden count.
    ///
    /// Found in the FLATTENED view and at every occurrence - see [`stated_numbers`], which records
    /// the two ways the per-line predecessor of this field read agreement out of prose it had not
    /// managed to look at.
    marker: &'static str,
}

/// TWO entries, and the table is short on purpose: a number is worth a gate when it carries an
/// ARGUMENT.
///
/// The row cap's does - it says the SQL leg is pinned across the corpus rather than in one
/// snapshot, and `39` was written when there were 39 and read as current at 63. The `pub trait`
/// one is a COMPARISON: the port table in `.agents/skills/engineering/rust/SKILL.md` lists what
/// the hexagon's interior owns, the tree holds more than that, and the gap is the whole point of
/// the paragraph - which a reader cannot see without both numbers.
///
/// Two numbers that are NOT here, both deliberately:
///
/// * `docs/crap.md` said `its 120 unit tests` at a real 170. DELETED from the prose rather than
///   gated, because it carried no argument - the sentence is about tests living in the crate they
///   cover, true at any count - and gating it would fail every branch that adds a test.
/// * *how many crates declare `[features]`* - proposed as an entry, declined for the same reason.
///   The sentence it would serve is *the `--all-features` habit is load-bearing now rather than
///   cheap foresight*, which is true at any count above zero, so the number adds nothing the word
///   *several* does not carry. The direction that would falsify it - no crate declaring a feature
///   at all - is held by the `no crate declares a feature` entry in [`CONTRADICTED`], whose
///   evidence is a manifest holding `[features]`.
pub(super) const COUNTS: &[Counted] = &[
    Counted {
        name: "SQL goldens carrying the row cap",
        // EVERY crate's snapshot directory, not just the one that had them when this entry was
        // written. `sutura-cli`'s 22 rendered-statement goldens carry the row cap and were outside
        // the glob, so the sentence said *pinned across the corpus* over 93 of 115 files and 22
        // goldens could have lost the cap with the gate silent. The claim is about the corpus, so
        // the glob is too - and a new crate's snapshots are inside it the day they land.
        over: &["crates/*/tests/snapshots/**"],
        holds: "LIMIT 10001",
        // FILES, and here that is a decision rather than the default it used to be: what is
        // claimed is how many goldens carry the row cap, and a golden carrying it twice is still
        // one golden.
        granularity: Granularity::Files,
        // `.agents/skills/**` is listed because that is where the sentence LIVES now. It was in
        // `AGENTS.md` until the router rewrite moved the invariants table into the tree, and this
        // list did not follow - so the only gated count in the repo was left counting the corpus
        // and comparing it to nothing. `PINS` was carried and this was not, which is this module's
        // own founding cause one layer up: the correction landed on one table and not its sibling.
        // The stated-side check is what makes the next one of those a failure.
        mentioned_in: &[".agents/skills/**", "AGENTS.md", "docs/**", "README.md"],
        marker: "SQL goldens read",
    },
    Counted {
        name: "`pub trait` declarations under `crates/*/src`",
        over: &["crates/*/src/**/*.rs"],
        holds: "pub trait ",
        // OCCURRENCES, and this is the entry that could not be held honestly before. Every
        // declaration sits in a file of its own today, so counting files would report the right
        // number BY COINCIDENCE and go green on the wrong one the day two of them share a file.
        //
        // The literal is the one the page cites, character for character, so the gate and the
        // command a reader is told to run answer the same question - a `pub trait ` inside a
        // comment or a string counts for both. Blanking comments would make the gate disagree with
        // the page, which is worse than the mention it would exclude.
        granularity: Granularity::Occurrences,
        mentioned_in: &[".agents/skills/**", "AGENTS.md", "CONTRIBUTING.md", "docs/**", "README.md"],
        marker: "`pub trait` declarations under",
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
fn flatten(text: &str) -> (String, Vec<usize>) {
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
        // The rule is live only while what refutes it is still there. A gate that kept forbidding
        // a sentence after it became true again is the same bug as a stale doc, one layer up.
        if !rule.evidence.iter().all(|e| e.stands(root)) {
            continue;
        }
        for rel in files {
            if !rule.only.is_empty() && !matches_any(rule.only, rel) {
                continue;
            }
            if matches_any(rule.except, rel) {
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

/// The integer at the end of `head`, if it ends in one.
fn trailing_number(head: &str) -> Option<u64> {
    let mut digits: Vec<char> = head.trim_end().chars().rev().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.reverse();
    digits.into_iter().collect::<String>().parse().ok()
}

/// Every statement of a number before `marker` in `text`: the line it starts on, and the number.
///
/// **Flattened, not per line, and that is a fix rather than a refinement.** This was
/// `count_before(line, marker)`, which had two blind spots a reviewer could not see from the
/// output because both of them look exactly like agreement: prose WRAPS, so a statement whose
/// number ended one line above its marker was not found at all; and `line.find` reads the FIRST
/// marker on a line, so a second statement on the same line was never read. Both are closed by
/// asking the flattened view for every occurrence - the same view [`contradicted_claims`] has
/// used from the start, for the same reason.
///
/// **What was rejected, because it would have failed correct prose.** The other candidate fix was
/// to require every page NAMING the marker to carry a number. `docs/implementation-plan-bigquery.md`
/// names `SQL goldens read` in order to explain the gate, with no number and correctly so, and a
/// gate that fails that sentence is one somebody switches off. Reading every occurrence needs no
/// exemption list.
fn stated_numbers(text: &str, marker: &str) -> Vec<(usize, u64)> {
    let (flat, lines) = flatten(text);
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(at) = flat.get(from..).and_then(|rest| rest.find(marker)) {
        let offset = from.saturating_add(at);
        if let Some(head) = flat.get(..offset)
            && let Some(number) = trailing_number(head)
        {
            found.push((lines.get(offset).copied().unwrap_or(1), number));
        }
        from = offset.saturating_add(marker.len().max(1));
    }
    found
}

/// What the tree holds, at the granularity this entry declares.
fn tally(root: &Path, all: &[String], counted: &Counted) -> u64 {
    let mut total = 0_u64;
    for rel in all {
        if !matches_any(counted.over, rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        total = total.saturating_add(counted.granularity.count_in(&text, counted.holds));
    }
    total
}

/// One place a count is written down: where, and what it says.
struct Stated {
    /// Repo-relative path of the page stating it.
    file: String,
    /// One-based line, so a message names the line a reader will open.
    line: usize,
    /// The number that page states.
    number: u64,
}

/// Every place this entry's number is actually written.
///
/// One implementation of *where is this number stated*, because the mismatch check and the
/// vacuity check are two questions about one answer and a second walk of the tree would be a
/// second thing to keep in step.
fn statements(root: &Path, files: &[String], counted: &Counted) -> Vec<Stated> {
    let mut found = Vec::new();
    for rel in files {
        if !matches_any(counted.mentioned_in, rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for (line, number) in stated_numbers(&text, counted.marker) {
            found.push(Stated {
                file: rel.clone(),
                line,
                number,
            });
        }
    }
    found
}

pub(super) fn count_mismatches(root: &Path, all: &[String], files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for counted in COUNTS {
        let actual = tally(root, all, counted);
        if actual == 0 {
            // Zero means the thing counted moved, not that the prose is right. A count check that
            // silently agreed with nothing would pass vacuously, which is worse than failing.
            problems.push(format!(
                "nothing in the tree matches the {} count (`{}` under {:?}) - the count moved, \
                 so this entry in COUNTS is measuring nothing",
                counted.name, counted.holds, counted.over
            ));
            continue;
        }
        let stated = statements(root, files, counted);
        for page in &stated {
            if page.number != actual {
                problems.push(format!(
                    "{}:{}: says {} {} - the tree has {actual}",
                    page.file, page.line, page.number, counted.name
                ));
            }
        }
        if stated.is_empty() {
            // The OTHER way this check goes vacuously green, and the one that actually happened:
            // the count is measured, nothing states it, and the gate agrees with silence. The
            // row-cap sentence lived in `AGENTS.md` until the router rewrite carried the
            // invariants table into `.agents/skills/`; `mentioned_in` did not follow, and a gate
            // that had caught `39` at 63 was left holding nothing. Failing here is the only way
            // that is visible, because a deleted sentence looks exactly like a correct one.
            problems.push(format!(
                "nothing under {:?} states the {} count - the number is derived and compared to \
                 nothing, so this entry in COUNTS is a gate over silence. State it before the \
                 marker `{}`, or delete the entry",
                counted.mentioned_in, counted.name, counted.marker
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::{CONTRADICTED, COUNTS, Evidence};

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
        }
    }

    #[test]
    fn the_number_read_is_the_one_immediately_before_the_marker() {
        use super::stated_numbers;
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
        use super::stated_numbers;
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
                super::tally(&root, &files, counted) > 0,
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
                !super::statements(&root, &files, counted).is_empty(),
                "no page under {:?} states the {} count before `{}`",
                counted.mentioned_in,
                counted.name,
                counted.marker
            );
        }
    }

    #[test]
    fn a_second_occurrence_in_one_file_counts_twice_only_where_the_entry_says_so() {
        use super::Granularity;
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
