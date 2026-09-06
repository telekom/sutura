//! Joining the two declarations, and the two witnesses that keep a count honest.
//!
//! Split from the module next door because the two halves fail differently, and because that file
//! reached 966 of an unexemptable 1000 lines - so the rule this repository applies to a threshold
//! lint applies to it: split the function rather than raise the number.
//!
//! **Two conservation laws, one per level, and each is a TYPE rather than a comparison somebody
//! remembered to write.** A count in a message is not evidence: this repository has shipped a gate
//! reporting `17 page(s), 16 generated` with 16 of 17 unscanned, and a census comparing two
//! hand-written lists that never read the tests it was about.
//!
//! | Level | Witness | What it refuses |
//! | --- | --- | --- |
//! | one decision per registry ENTRY | [`Reconciled::of`] | a verdict over a subset of what the scan found |
//! | one decision per BINDING | [`Classified::of`] | a binding dropped on the floor, which `matched.insert` did silently |
//!
//! The second arrived after review measured the first not covering it: a second binding for one
//! data system overwrote the first in a `BTreeMap`, and *which* verdict came out was decided by the
//! file listing's sort order - `tests/aaa.rs` gave a silent green whose only tell was the file
//! count moving 239 to 240, and `tests/zz.rs` gave a failure naming the wrong file as the sole
//! binding. Both are the same absent witness, one level below the one that was there.

use std::collections::BTreeMap;

use super::{BINDING_FILE, BINDING_MODULE, Binding, Entry, owner, scan};

/// A registered data system that is deliberately not bound, and why.
///
/// Declared rather than excluded from the comparison, for the reason `check-bounded-wait` declares
/// its one allowance: an exclusion hides an entry and a declaration classifies it - and then
/// [`inert`] makes the classification answer for itself.
#[derive(Debug)]
pub(super) struct Unbound {
    /// The registry cell's name, matched exactly.
    pub(super) name: &'static str,
    /// The crate that entry derives. Held as well as the name so that an entry cannot survive the
    /// adapter type moving to another crate, which is exactly when the reason below stops applying.
    pub(super) crate_name: &'static str,
    /// What is registered and unbound. Printed, so a reader hitting the inert-exemption failure
    /// knows what the entry was about.
    pub(super) what: &'static str,
    /// Why that is not a hole. Printed, because an exemption whose reason is unstated is
    /// indistinguishable from an oversight.
    pub(super) why: &'static str,
}

/// Every registered data system this gate accepts as unbound.
///
/// **EMPTY, and that is the state this gate was built to reach.** It shipped with one entry -
/// `postgres`, registered in the golden matrix and carrying no binding because the packs had no way
/// to say *no tier is up here* that was not a pass - and `docs/adr/0012`'s *one registration, not
/// two* was violated by exactly that entry. `telekom/sutura#348` gave the harness
/// `sutura_conformance::Fixture`, `crates/sutura-exec-postgres/tests/conformance.rs` binds it, and
/// so the entry is gone rather than reworded. An exemption for something that is bound after all is
/// itself a failure here ([`inert`]), so this list could not have been left behind.
///
/// **Adding one back is an architecture decision** - the sentence `BLOCKING` in
/// `xtask/src/bounded_wait.rs` carries for the same reason. The diff is where the argument happens,
/// and an entry that stops being true fails the gate rather than quietly widening it. The mechanism
/// stays with the list empty on purpose: the alternative to a declared exemption is an
/// EXCLUSION, which hides an entry instead of classifying it, and the next adapter that cannot be
/// bound will need this and not that.
pub(super) const UNBOUND: &[Unbound] = &[];

/// What the tree says about one registry entry. Two states are held and two are the defect.
#[derive(Debug)]
pub(super) enum Held {
    /// Bound, in the file and module the selectors need.
    Bound(Binding),
    /// Bound, and not where a selector can reach it.
    Misbound(String),
    /// Declared unbound, by this entry.
    Exempt(&'static Unbound),
    /// Neither bound nor declared, which is the hole this gate exists for.
    Unheld,
}

/// A binding that no registry entry claims, or claims twice.
pub(super) enum Stray {
    /// In the crate that DEFINES the packs: the harness's own fakes, which are not a data system
    /// and must not be. Printed on green rather than hidden, so a fake that moved is visible.
    Fake(Binding),
    /// In a crate the registry names no data system from - so nothing schedules it, and the CI
    /// matrix `docs/adr/0012` emits from the registry cannot see it.
    Unregistered(Binding),
    /// In a registry crate, under a name that crate's entry does not carry - so the selector the
    /// matrix would spell names no test. Carries the name the registry does declare there.
    Misnamed(Binding, String),
    /// A SECOND binding for a name already bound, carrying the one already matched. Reported
    /// rather than overwritten, because the per-adapter selector matches by test NAME across
    /// binaries: two of them run one tier twice and neither reader is told.
    Duplicate(Binding, Binding),
}

/// One registry entry and the decision made about it. The unit [`Reconciled`] refuses to be short of.
pub(super) type Decision = (Entry, Held);

/// Which binding was matched to which registry name.
pub(super) type Matched = BTreeMap<String, Binding>;

/// Every binding, split into the ones a registry entry claims and the ones no entry does.
///
/// **The witness one level below [`Reconciled`], and the reason it is a type.** The split is a
/// partition of the binding set, so `matched + strays` is the number of bindings found or the split
/// lost one - and losing one was not hypothetical: `matched.insert` returns the value it replaced
/// and the first version of this gate dropped it.
pub(super) struct Classified {
    matched: Matched,
    strays: Vec<Stray>,
}

impl Classified {
    /// Which binding belongs to which entry, and which belongs to none.
    ///
    /// Keyed on the crate FIRST and the name second, so a binding written in a registry crate under
    /// the wrong name is reported as that rather than as two separate absences.
    ///
    /// **The conservation check cannot fire while [`Stray::Duplicate`] exists**, and it is kept for
    /// the reason [`Reconciled::of`]'s empty-registry arm is: it belongs to the witness rather than
    /// to the loop, so the next author who reaches for `insert` and discards what it returned gets
    /// a failure instead of a silent green.
    pub(super) fn of(entries: &[Entry], harness: &str, bindings: &[Binding]) -> Result<Self, String> {
        let mut matched = Matched::new();
        let mut strays = Vec::new();
        for binding in bindings {
            let dir = owner(&binding.path).unwrap_or_default();
            if dir == harness {
                strays.push(Stray::Fake(binding.clone()));
                continue;
            }
            let here: Vec<&Entry> = entries.iter().filter(|entry| entry.crate_dir == dir).collect();
            if here.is_empty() {
                strays.push(Stray::Unregistered(binding.clone()));
            } else if here.iter().any(|entry| entry.name == binding.invocation.adapter) {
                // The first is KEPT and the second reported, rather than the second overwriting
                // the first: both are named in the message either way, so no reader is sent to a
                // file that is not the whole story.
                if let Some(first) = matched.get(&binding.invocation.adapter) {
                    strays.push(Stray::Duplicate(first.clone(), binding.clone()));
                } else {
                    drop(matched.insert(binding.invocation.adapter.clone(), binding.clone()));
                }
            } else {
                let expected = here.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>().join("`, `");
                strays.push(Stray::Misnamed(binding.clone(), expected));
            }
        }
        conserved(bindings.len(), matched.len(), strays.len())?;
        Ok(Self { matched, strays })
    }

    /// The name-to-binding map the entry-side decision is taken against.
    pub(super) const fn matched(&self) -> &Matched {
        &self.matched
    }

    /// Every binding no registry entry claims.
    pub(super) fn strays(&self) -> &[Stray] {
        &self.strays
    }
}

/// The conservation law, as a function so its refusal can be exercised.
///
/// [`Classified::of`] cannot produce a lossy split while [`Stray::Duplicate`] exists, and a
/// witness nobody can call is a witness nobody can trust - `Reconciled::of`'s empty-registry arm
/// is the same shape and this gate's own header says so rather than claiming the arm fires.
fn conserved(found: usize, matched: usize, strays: usize) -> Result<(), String> {
    let accounted = matched.saturating_add(strays);
    if accounted == found {
        return Ok(());
    }
    Err(format!(
        "{found} binding(s) were found and {accounted} accounted for - a binding this gate dropped \
         is one it did not judge, and which verdict a dropped binding produces is decided by the \
         file listing's sort order rather than by the tree"
    ))
}

/// Every registry entry, paired with what the tree says about it.
///
/// **The pairing is the witness.** [`Self::of`] refuses a decision list that is not one decision
/// per entry found, so a scan that judged fewer members than it found cannot reach the line that
/// prints a number.
#[derive(Debug)]
pub(super) struct Reconciled {
    pub(super) judged: Vec<Decision>,
}

impl Reconciled {
    /// Pairs the entries with their decisions, refusing anything that is not one-for-one.
    pub(super) fn of(entries: Vec<Entry>, decided: Vec<Held>) -> Result<Self, String> {
        if entries.is_empty() {
            return Err(String::from(
                "the registry declares no data system, so every comparison below would be about nothing",
            ));
        }
        if entries.len() != decided.len() {
            return Err(format!(
                "{} registered data system(s) were found and {} judged - a verdict over a subset is \
                 not a verdict, so the count is refused rather than printed",
                entries.len(),
                decided.len()
            ));
        }
        Ok(Self {
            judged: entries.into_iter().zip(decided).collect(),
        })
    }

    /// Every way the two declarations disagree, as one message each.
    pub(super) fn problems(&self, strays: &[Stray]) -> Vec<String> {
        let mut problems: Vec<String> = self
            .judged
            .iter()
            .filter_map(|(entry, held)| match held {
                Held::Bound(..) | Held::Exempt(_) => None,
                Held::Misbound(why) => Some(why.clone()),
                Held::Unheld => Some(format!(
                    "`{}` is registered as a data system ({}) and no crate binds it to the \
                     conformance packs - {}{BINDING_FILE} holds no `{}`",
                    entry.name,
                    entry.adapter,
                    entry.crate_dir,
                    scan::BINDING
                )),
            })
            .collect();
        problems.extend(strays.iter().filter_map(Stray::problem));
        problems.extend(inert(UNBOUND, &self.judged));
        problems
    }

    /// The entries that are bound, with the selector each one's binding makes spellable.
    pub(super) fn bound(&self) -> impl Iterator<Item = (&Entry, &Binding)> {
        self.judged.iter().filter_map(|(entry, held)| match held {
            Held::Bound(binding) => Some((entry, binding)),
            _ => None,
        })
    }

    /// The entries declared unbound.
    pub(super) fn exempt(&self) -> impl Iterator<Item = (&Entry, &'static Unbound)> {
        self.judged.iter().filter_map(|(entry, held)| match held {
            Held::Exempt(allowance) => Some((entry, *allowance)),
            _ => None,
        })
    }
}

impl Stray {
    /// The message, for the kinds that are a defect. A fake in the harness's own crate is not one -
    /// unless it took the binding FILE name, which is the one placement rule the harness shares.
    fn problem(&self) -> Option<String> {
        match self {
            Self::Fake(binding) => binding.path.ends_with(BINDING_FILE).then(|| {
                format!(
                    "{}:{} is a fake in the crate that DEFINES the packs, written in {BINDING_FILE} - so \
                 `-E 'binary({BINDING_MODULE})'` selects the harness's own fake cells alongside \
                 every adapter's tier. A fake belongs in any other file of that crate's `tests/`",
                    binding.path, binding.invocation.line
                )
            }),
            Self::Unregistered(binding) => Some(format!(
                "{}:{} binds `{}` to the packs and the registry names no data system from that \
                 crate - so no CI matrix emitted from the registry schedules it, which is *one \
                 registration, not two* failing in the other direction",
                binding.path, binding.invocation.line, binding.invocation.adapter
            )),
            Self::Misnamed(binding, expected) => Some(format!(
                "{}:{} binds `{}` and the registry calls that crate's data system `{expected}` - \
                 the emitted tests are `{}::<behaviour>` and a matrix keyed on the registry would \
                 select nothing",
                binding.path,
                binding.invocation.line,
                binding.invocation.adapter,
                binding.invocation.selector()
            )),
            Self::Duplicate(first, second) => Some(format!(
                "`{}` is bound twice - {}:{} and {}:{} - and one registered name is one binding: \
                 `-E 'test({})'` matches by test NAME across binaries, so both tiers run under the \
                 per-adapter selector and the run's own count is the only tell. Delete one",
                second.invocation.adapter,
                first.path,
                first.invocation.line,
                second.path,
                second.invocation.line,
                second.invocation.selector()
            )),
        }
    }
}

/// Every declared exemption that no longer excuses anything.
///
/// Three ways, and each one widens what is permitted while reading as a considered decision: an
/// entry for a name the registry does not carry, an entry naming the wrong crate for a name it
/// does, and an entry excusing something that is bound after all.
///
/// **The list is a PARAMETER rather than [`UNBOUND`] read directly**, and that is what keeps this
/// function provable now that the constant is empty: every caller in the gate passes `UNBOUND`, and
/// the tests pass a list they built - so all three arms stay provoked by a test rather than by
/// whichever exemption happened to be declared. A check whose only fixture is live data stops being
/// a check the day that data goes away, which is the shape this repository has already paid for.
fn inert(declared: &[Unbound], judged: &[Decision]) -> Vec<String> {
    let mut problems = Vec::new();
    for allowance in declared {
        let Some((entry, held)) = judged.iter().find(|(entry, _)| entry.name == allowance.name) else {
            problems.push(format!(
                "`{}` is declared here as deliberately unbound ({}) and the registry carries no data \
                 system of that name - delete the entry rather than leaving it to excuse something \
                 nobody registered",
                allowance.name, allowance.what
            ));
            continue;
        };
        if entry.crate_name != allowance.crate_name {
            problems.push(format!(
                "`{}` is declared here as unbound in `{}` and the registry derives `{}` for it - the \
                 reason an entry gives is about a crate, so an entry that names another one is stale",
                allowance.name, allowance.crate_name, entry.crate_name
            ));
        }
        if let Held::Bound(binding) = held {
            problems.push(format!(
                "`{}` is declared here as deliberately unbound ({}) and {} binds it - delete the \
                 entry rather than leaving it to excuse a binding that exists",
                allowance.name, allowance.what, binding.path
            ));
        }
    }
    problems
}

/// What the tree says about one entry: bound where a selector reaches it, declared, or unheld.
pub(super) fn decide(entry: &Entry, matched: &Matched) -> Held {
    matched.get(&entry.name).map_or_else(
        || {
            UNBOUND
                .iter()
                .find(|allowance| allowance.name == entry.name)
                .map_or(Held::Unheld, Held::Exempt)
        },
        |binding| misplacement(entry, binding).map_or_else(|| Held::Bound(binding.clone()), Held::Misbound),
    )
}

/// Why this binding is not where the tier selectors can reach it, if it is not.
///
/// The two properties `telekom/sutura#135` rests on, and the macro's own documentation said
/// nothing enforced either of them.
fn misplacement(entry: &Entry, binding: &Binding) -> Option<String> {
    let expected = format!("{}{BINDING_FILE}", entry.crate_dir);
    let (path, invocation) = (&binding.path, &binding.invocation);
    if *path != expected {
        return Some(format!(
            "`{}` is bound at {path}:{} and a tier is selected by binary, so it has to be \
             {expected} for `-E 'binary({BINDING_MODULE})'` to name it",
            entry.name, invocation.line
        ));
    }
    if !matches!(invocation.module.as_slice(), [only] if only == BINDING_MODULE) {
        return Some(format!(
            "`{}` is bound at {path}:{} inside `{}`, so its tests are `{}::<behaviour>` and the \
             per-adapter selector `-E 'test({BINDING_MODULE}::{})'` names nothing - the invocation \
             belongs in `mod {BINDING_MODULE}`",
            entry.name,
            invocation.line,
            if invocation.module.is_empty() {
                String::from("the file's own root")
            } else {
                invocation.module.join("::")
            },
            invocation.selector(),
            entry.name
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Binding, Classified, Entry, Held, Reconciled, Stray, UNBOUND, Unbound, decide, inert, misplacement, scan};

    fn entry(name: &str, crate_name: &str) -> Entry {
        Entry {
            name: String::from(name),
            adapter: format!("{}::Warehouse", crate_name.replace('-', "_")),
            crate_name: String::from(crate_name),
            crate_dir: format!("crates/{crate_name}/"),
        }
    }

    fn invocation(adapter: &str, module: &[&str]) -> scan::Invocation {
        scan::Invocation {
            line: 42,
            adapter: String::from(adapter),
            module: module.iter().map(|part| String::from(*part)).collect(),
        }
    }

    fn binding(adapter: &str, path: &str, module: &[&str]) -> Binding {
        Binding {
            path: String::from(path),
            invocation: invocation(adapter, module),
        }
    }

    fn bound_at(adapter: &str, path: &str, module: &[&str]) -> super::Matched {
        let mut matched = super::Matched::new();
        drop(matched.insert(String::from(adapter), binding(adapter, path, module)));
        matched
    }

    /// A registered adapter with no binding and no exemption is a failure that NAMES it.
    #[test]
    fn an_unbound_registered_adapter_is_reported_by_name() {
        let entries = vec![entry("ghost", "sutura-exec-ghost")];
        let decided = vec![Held::Unheld];
        let reconciled = Reconciled::of(entries, decided).expect("one entry, one decision");
        let problems = reconciled.problems(&[]);
        let named = problems
            .iter()
            .find(|why| why.contains("`ghost` is registered"))
            .unwrap_or_else(|| panic!("the unbound entry is named: {problems:?}"));
        assert!(named.contains("crates/sutura-exec-ghost/tests/conformance.rs"), "{named}");
        // ONE problem, and the number moved when `telekom/sutura#348` deleted the `postgres`
        // exemption: it used to be two, the second being the inert-exemption half firing over this
        // fixture's registry. Both are still reported in one run where both exist - a gate that
        // knew two numbers and printed one cost a round trip (`telekom/sutura#311`) - and the arms
        // that prove it are the three `inert` cells below, which build their own exemption.
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    /// **The witness.** A verdict that judged fewer members than it found is refused rather than
    /// printed: this repository has shipped `17 page(s), 16 generated` with 16 of 17 unscanned.
    #[test]
    fn judging_fewer_entries_than_were_found_is_refused() {
        let entries = vec![entry("one", "sutura-exec-one"), entry("two", "sutura-exec-two")];
        let why = Reconciled::of(entries, vec![Held::Unheld]).expect_err("a subset is not a verdict");
        assert!(why.contains("2 registered data system(s) were found and 1 judged"), "{why}");
    }

    /// An empty registry is a failure, not a green run over nothing.
    #[test]
    fn an_empty_registry_is_refused() {
        let why = Reconciled::of(Vec::new(), Vec::new()).expect_err("an empty adapter set is refused");
        assert!(why.contains("declares no data system"), "{why}");
    }

    /// **The second witness, and the defect it was added for.** A second binding for one data
    /// system used to overwrite the first, so the verdict depended on the file listing's sort
    /// order: `tests/aaa.rs` gave a silent green and `tests/zz.rs` a failure naming the wrong
    /// file. Now both files are named in one message, whichever way they sort.
    #[test]
    fn a_second_binding_for_one_data_system_is_reported_with_both_files() {
        let entries = vec![entry("duckdb", "sutura-exec-duckdb")];
        let first = binding("duckdb", "crates/sutura-exec-duckdb/tests/aaa.rs", &["conformance"]);
        let second = binding("duckdb", "crates/sutura-exec-duckdb/tests/conformance.rs", &["conformance"]);
        let classified = Classified::of(&entries, "crates/sutura-conformance/", &[first, second])
            .expect("the split conserves the binding set");
        let problems: Vec<String> = classified.strays().iter().filter_map(Stray::problem).collect();
        let named = problems
            .first()
            .unwrap_or_else(|| panic!("the duplicate is named: {problems:?}"));
        assert!(named.contains("bound twice"), "{named}");
        assert!(
            named.contains("tests/aaa.rs:42") && named.contains("tests/conformance.rs:42"),
            "{named}"
        );
        // And the FIRST is what the entry-side decision is taken against, so the verdict is not a
        // reading of the listing's order.
        assert!(classified.matched().len() == 1, "{}", classified.matched().len());
    }

    /// The conservation law itself. Called directly, because `of` cannot produce a lossy split
    /// while the duplicate arm exists - and the law is what holds when a future author reaches for
    /// `insert` again and discards what it returned.
    #[test]
    fn a_split_that_loses_a_binding_is_refused() {
        let why = super::conserved(2, 1, 0).expect_err("one binding of two is not a split");
        assert!(why.contains("2 binding(s) were found and 1 accounted for"), "{why}");
        assert!(why.contains("sort order"), "{why}");
        super::conserved(2, 1, 1).expect("a matched and a stray account for two");
    }

    /// A binding outside the file a `binary()` filter names is reported, with the path it needs.
    #[test]
    fn a_binding_in_another_file_is_not_where_a_tier_selector_reaches_it() {
        let subject = entry("duckdb", "sutura-exec-duckdb");
        let matched = bound_at("duckdb", "crates/sutura-exec-duckdb/tests/packs.rs", &["conformance"]);
        let Held::Misbound(why) = decide(&subject, &matched) else {
            panic!("a binding in the wrong file is misplaced")
        };
        assert!(why.contains("tests/packs.rs"), "{why}");
        assert!(why.contains("crates/sutura-exec-duckdb/tests/conformance.rs"), "{why}");
    }

    /// A binding outside `mod conformance` is reported with the selector it actually produces.
    #[test]
    fn a_binding_outside_the_wrapper_module_is_not_selectable_per_adapter() {
        let subject = entry("duckdb", "sutura-exec-duckdb");
        let matched = bound_at("duckdb", "crates/sutura-exec-duckdb/tests/conformance.rs", &["packs"]);
        let Held::Misbound(why) = decide(&subject, &matched) else {
            panic!("a binding outside the wrapper module is misplaced")
        };
        assert!(why.contains("inside `packs`"), "{why}");
        assert!(why.contains("packs::duckdb"), "{why}");
    }

    /// The bound case, and the selector is DERIVED from where the invocation sits.
    #[test]
    fn a_binding_in_the_right_place_is_held_and_carries_its_selector() {
        let subject = entry("duckdb", "sutura-exec-duckdb");
        let here = binding("duckdb", "crates/sutura-exec-duckdb/tests/conformance.rs", &["conformance"]);
        assert!(misplacement(&subject, &here).is_none());
        let matched = bound_at("duckdb", "crates/sutura-exec-duckdb/tests/conformance.rs", &["conformance"]);
        let Held::Bound(found) = decide(&subject, &matched) else {
            panic!("a binding in the right place is bound")
        };
        assert_eq!(found.invocation.selector(), "conformance::duckdb");
    }

    /// One exemption, built here rather than read out of [`UNBOUND`].
    ///
    /// **Which is what keeps the three arms below provable now that the constant is empty.** The
    /// earlier version of each of them started `UNBOUND.first().expect("one declared exemption")`,
    /// so deleting the only entry - the thing `telekom/sutura#348` exists to do - would have turned
    /// this gate's own inert-exemption checks into three panicking cells. A check whose only
    /// fixture is live data stops being a check the day the data goes.
    const DECLARED: Unbound = Unbound {
        name: "ghost",
        crate_name: "sutura-exec-ghost",
        what: "registered in the golden matrix and carrying no `tests/conformance.rs`",
        why: "a fixture for this file, so the arms below are provoked by a test rather than by \
              whichever exemption happens to be declared",
    };

    /// **The inert exemption**, which is the `max-lines` defect this gate must not repeat: an
    /// entry excusing something that is bound after all is a failure, and it is reported in the
    /// same run as everything else the gate knows.
    #[test]
    fn an_exemption_for_something_that_is_bound_is_reported_as_inert() {
        let judged = vec![(
            entry(DECLARED.name, DECLARED.crate_name),
            Held::Bound(binding(
                DECLARED.name,
                &format!("crates/{}/tests/conformance.rs", DECLARED.crate_name),
                &["conformance"],
            )),
        )];
        let problems = inert(&[DECLARED], &judged);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems.first().is_some_and(|why| why.contains("delete the entry")),
            "{problems:?}"
        );
    }

    /// An exemption for something the registry does not carry is inert too, and in the direction
    /// a stale entry actually goes: the registry moved and the exemption stayed.
    #[test]
    fn an_exemption_for_an_unregistered_name_is_reported_as_inert() {
        let problems = inert(&[DECLARED], &[(entry("somebody-else", "sutura-exec-other"), Held::Unheld)]);
        assert!(
            problems.iter().any(|why| why.contains("carries no data system of that name")),
            "{problems:?}"
        );
    }

    /// An exemption naming the wrong crate for a registered name is inert: the reason an entry
    /// gives is about a crate, so it stops applying when the adapter type moves.
    #[test]
    fn an_exemption_naming_the_wrong_crate_is_reported_as_inert() {
        let problems = inert(&[DECLARED], &[(entry(DECLARED.name, "sutura-exec-moved"), Held::Unheld)]);
        assert!(
            problems
                .iter()
                .any(|why| why.contains("the reason an entry gives is about a crate")),
            "{problems:?}"
        );
    }

    /// **Nothing is exempt today, and that is asserted rather than assumed.**
    ///
    /// The gate shipped with `postgres` declared unbound because the packs could not say *no tier
    /// is up here*; `telekom/sutura#348` gave them `sutura_conformance::Fixture` and the adapter
    /// binds them from its own crate, so an entry here now would be an exemption for something that
    /// is bound - which [`inert`] reports. This cell is what makes re-adding one deliberate: it
    /// reddens, and the diff is where the argument has to be.
    #[test]
    fn no_registered_data_system_is_declared_unbound() {
        assert!(
            UNBOUND.is_empty(),
            "an exemption is an architecture decision, and this cell is where it is argued: {:?}",
            UNBOUND.iter().map(|allowance| allowance.name).collect::<Vec<_>>()
        );
    }

    /// A binding in a crate the registry names nothing from is the other direction of *one
    /// registration, not two*: coverage no emitted matrix schedules.
    #[test]
    fn a_binding_the_registry_does_not_name_is_reported() {
        let stray = Stray::Unregistered(binding(
            "bigquery",
            "crates/sutura-exec-bigquery/tests/conformance.rs",
            &["conformance"],
        ));
        let why = stray.problem().expect("an unregistered binding is a problem");
        assert!(why.contains("the registry names no data system from that crate"), "{why}");
    }

    /// The harness's own fakes are not a data system and are not reported as one - they are
    /// PRINTED on green instead, so a fake that moved crate is visible rather than silent.
    #[test]
    fn the_packs_crates_own_fake_bindings_are_not_a_violation() {
        let stray = Stray::Fake(binding(
            "a_fake_that_executes_legs",
            "crates/sutura-conformance/tests/bound.rs",
            &["conformance"],
        ));
        assert!(stray.problem().is_none());
    }

    /// **The one placement rule the harness does share**, and the reason *"and nothing else"* was
    /// an overstatement until this arm existed: a fake written in `tests/conformance.rs` is
    /// selected by `binary(conformance)` alongside every adapter's tier.
    #[test]
    fn a_fake_that_takes_the_binding_file_name_is_a_violation() {
        let stray = Stray::Fake(binding(
            "a_third_fake",
            "crates/sutura-conformance/tests/conformance.rs",
            &["conformance"],
        ));
        let why = stray.problem().expect("a fake in the binding file is a problem");
        assert!(why.contains("binary(conformance)"), "{why}");
    }

    /// Whatever is declared carries a reason. Vacuous while [`UNBOUND`] is empty, and kept for the
    /// entry that is added back: the cell above is what says the list is empty on purpose.
    #[test]
    fn every_declared_exemption_carries_its_reason() {
        for allowance in UNBOUND.iter().chain(core::iter::once(&DECLARED)) {
            assert!(!allowance.what.is_empty(), "{} has no `what`", allowance.name);
            assert!(!allowance.why.is_empty(), "{} has no `why`", allowance.name);
            assert!(
                allowance.crate_name.starts_with("sutura-"),
                "{} names no crate",
                allowance.name
            );
        }
    }
}
