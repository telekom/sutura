//! Catalog-only conformance, behind the default-off `compile` feature.
//!
//! Fixtures supply a catalog, independent expected values and questions. These packs own every
//! comparison; no warehouse, filesystem corpus or callback implementing an assertion is required.
//! Golden catalogs owe the oracle and compile cases. Declaring catalogs owe declaration fidelity
//! and repeat-load determinism, not the golden model. [`Golden`] holds that distinction for direct
//! calls; [`crate::compile_packs`] additionally checks the binding's tag at compile time.
//!
//! A case pins the serialized plan (JSON value equality, not whitespace), or the exact typed
//! refusal. Only after that comparison do we render, comparing statement, source and parameters for
//! each member of `sutura_sql::dialect::ALL`; federated statements are ordered fact then lookup. This does not
//! prove a server accepts SQL, execute rows, establish identity, or prove `ALL` exhausts its enum.
//! Expectations produced by the compiler under test are not an independent oracle. No real catalog
//! is registered here yet; the existing app goldens and their snapshots remain untouched.

use sutura_domain::capabilities::{MetadataCapabilities, UnfaithfulDeclaration};
use sutura_domain::pinned::{CatalogKind, PinnedDefinitions, SemanticCatalog};
use sutura_domain::query::{Query, RefusalReason};
use sutura_semantic::{CompileFailure, Compiled};
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};

/// An adapter whose own declaration permits the golden-only checks. This borrows, never clones,
/// the adapter; a declaring adapter cannot acquire this witness through a public field.
pub struct Golden<'a, C> {
    catalog: &'a C,
}

/// A declaring catalog cannot be held to somebody else's complete model.
#[derive(Debug, thiserror::Error)]
#[error("golden conformance requires CatalogKind::Golden, found {actual:?}")]
pub struct NotGolden {
    actual: CatalogKind,
}

impl<'a, C> Golden<'a, C>
where
    C: SemanticCatalog,
{
    pub const fn new(catalog: &'a C) -> Result<Self, NotGolden> {
        match C::KIND {
            CatalogKind::Golden => Ok(Self { catalog }),
            CatalogKind::Declaring => Err(NotGolden { actual: C::KIND }),
        }
    }
}

/// One dialect's independently expected statements. A mono plan has one, a federated plan two.
pub struct Rendering {
    dialect: Dialect,
    statements: Vec<GeneratedQuery>,
}

impl Rendering {
    pub const fn new(dialect: Dialect, statements: Vec<GeneratedQuery>) -> Self {
        Self { dialect, statements }
    }
}

/// Expected values, not a second serialized format: plan values are the domain's own serialization.
pub enum Expected {
    Planned(serde_json::Value),
    Federated(serde_json::Value),
    Refused(RefusalReason),
}

/// One named question and the independently authored values it must produce.
pub struct Case {
    name: &'static str,
    question: Query,
    expected: Expected,
    renderings: Vec<Rendering>,
}

impl Case {
    pub const fn new(name: &'static str, question: Query, expected: Expected, renderings: Vec<Rendering>) -> Self {
        Self {
            name,
            question,
            expected,
            renderings,
        }
    }
}

/// Which contract failed. Catalog, compiler and renderer errors retain their typed causes.
#[derive(Debug, thiserror::Error)]
pub enum Fault<E>
where
    E: core::error::Error + 'static,
{
    #[error("the catalog could not be loaded")]
    Load(#[source] E),
    #[error("the catalog's declaration differs from its content")]
    Declaration(#[source] UnfaithfulDeclaration),
    #[error("loading the same catalog twice changed its digest")]
    Unstable,
    #[error("the golden catalog differs from the independent definitions or knowledge")]
    Oracle,
    #[error("the compile corpus is empty")]
    EmptyCorpus,
    #[error("case {case} could not be compiled")]
    Compile {
        case: &'static str,
        #[source]
        cause: CompileFailure,
    },
    #[error("case {case} could not serialize its plan")]
    Serialize {
        case: &'static str,
        #[source]
        cause: serde_json::Error,
    },
    #[error("case {case} produced a different plan or refusal")]
    Outcome { case: &'static str },
    #[error("case {case} must name every declared dialect once, or none for a refusal")]
    Dialects { case: &'static str },
    #[error("case {case} could not render for {dialect}")]
    Render {
        case: &'static str,
        dialect: Dialect,
        #[source]
        cause: GenerateError,
    },
    #[error("case {case} differs in statement, source or bind parameters for {dialect}")]
    Statement { case: &'static str, dialect: Dialect },
}

/// The result of one catalog conformance check.
pub type Checked<E> = Result<(), Fault<E>>;

/// Universal: both under-declaration and promised-but-absent content are faults.
pub fn declaration_matches<C>(catalog: &C) -> Checked<C::Error>
where
    C: SemanticCatalog,
{
    let pinned = catalog.load().map_err(Fault::Load)?;
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    C::capabilities().checked_against(&produced).map_err(Fault::Declaration)
}

/// Universal: repeat-load determinism only, not invariance under reformatting or changed inputs.
pub fn repeats_its_digest<C>(catalog: &C) -> Checked<C::Error>
where
    C: SemanticCatalog,
{
    let first = catalog.load().map_err(Fault::Load)?;
    let second = catalog.load().map_err(Fault::Load)?;
    if first.digest() != second.digest() {
        return Err(Fault::Unstable);
    }
    Ok(())
}

/// Golden-only: exact definitions and knowledge, without requiring the fixture's version or source
/// contribution manifest to be the oracle's. Prose is compared, not erased.
pub fn agrees_with_oracle<C>(golden: &Golden<'_, C>, oracle: &PinnedDefinitions) -> Checked<C::Error>
where
    C: SemanticCatalog,
{
    let pinned = golden.catalog.load().map_err(Fault::Load)?;
    if pinned.definitions() != oracle.definitions() || pinned.knowledge() != oracle.knowledge() {
        return Err(Fault::Oracle);
    }
    Ok(())
}

/// Golden-only: compare the plan/refusal before touching the renderer, then every dialect's output.
/// An empty corpus and missing or repeated dialects cannot silently reduce the tested population.
pub fn matches_cases<C>(golden: &Golden<'_, C>, cases: &[Case]) -> Checked<C::Error>
where
    C: SemanticCatalog,
{
    if cases.is_empty() {
        return Err(Fault::EmptyCorpus);
    }
    let pinned = golden.catalog.load().map_err(Fault::Load)?;
    for case in cases {
        let compiled =
            sutura_semantic::compile(&case.question, &pinned).map_err(|cause| Fault::Compile { case: case.name, cause })?;
        let same = match (&compiled, &case.expected) {
            (Compiled::Planned { plan }, Expected::Planned(expected)) => {
                serde_json::to_value(plan).map_err(|cause| Fault::Serialize { case: case.name, cause })? == *expected
            }
            (Compiled::Federated { plan }, Expected::Federated(expected)) => {
                serde_json::to_value(plan).map_err(|cause| Fault::Serialize { case: case.name, cause })? == *expected
            }
            (Compiled::Refused { reason }, Expected::Refused(expected)) => reason == expected,
            _ => false,
        };
        if !same {
            return Err(Fault::Outcome { case: case.name });
        }
        statements_match::<C>(&compiled, case)?;
    }
    Ok(())
}

/// The same compiled value that passed the plan check is what is rendered, never a second compile.
fn statements_match<C>(compiled: &Compiled, case: &Case) -> Checked<C::Error>
where
    C: SemanticCatalog,
{
    let dialects: &[Dialect] = match compiled {
        Compiled::Refused { .. } => &[],
        Compiled::Planned { .. } | Compiled::Federated { .. } => sutura_sql::dialect::ALL,
    };
    if case.renderings.len() != dialects.len()
        || dialects
            .iter()
            .any(|dialect| case.renderings.iter().filter(|r| r.dialect == *dialect).count() != 1)
    {
        return Err(Fault::Dialects { case: case.name });
    }
    for rendering in &case.renderings {
        let actual = match compiled {
            Compiled::Planned { plan } => sutura_sql::generate(plan, rendering.dialect).map(|statement| vec![statement]),
            Compiled::Federated { plan } => plan
                .legs()
                .into_iter()
                .map(|leg| sutura_sql::generate_leg(leg, rendering.dialect))
                .collect(),
            Compiled::Refused { .. } => Ok(Vec::new()),
        }
        .map_err(|cause| Fault::Render {
            case: case.name,
            dialect: rendering.dialect,
            cause,
        })?;
        if actual != rendering.statements {
            return Err(Fault::Statement {
                case: case.name,
                dialect: rendering.dialect,
            });
        }
    }
    Ok(())
}

/// Binds named catalog checks. `golden` additionally takes `oracle` and `cases` constructors;
/// `declaring` deliberately cannot supply either. The tag must agree with `SemanticCatalog::KIND`.
///
/// The macro and public calls are exercised by this crate's integration test; registry coverage is
/// not inferred from these invocations and remains the later registration work.
#[macro_export]
macro_rules! compile_packs {
    (adapter: $name:ident, catalog: $catalog:ty, open: $open:path, declaring $(,)?) => {
        #[cfg(test)]
        mod $name {
            const _: () = assert!(matches!(<$catalog as $crate::sutura_domain::pinned::SemanticCatalog>::KIND,
                $crate::sutura_domain::pinned::CatalogKind::Declaring));
            $crate::compile_packs!(@universal $catalog, $open);
        }
    };
    (adapter: $name:ident, catalog: $catalog:ty, open: $open:path, golden, oracle: $oracle:path, cases: $cases:path $(,)?) => {
        #[cfg(test)]
        mod $name {
            const _: () = assert!(matches!(<$catalog as $crate::sutura_domain::pinned::SemanticCatalog>::KIND,
                $crate::sutura_domain::pinned::CatalogKind::Golden));
            $crate::compile_packs!(@universal $catalog, $open);
            #[test]
            fn the_catalog_agrees_with_the_oracle() {
                let catalog: $catalog = $open();
                let golden = $crate::compile::Golden::new(&catalog).expect("the binding declares a golden catalog");
                $crate::compile::agrees_with_oracle(&golden, &$oracle()).expect("the golden catalog agrees");
            }
            #[test]
            fn the_plans_refusals_and_statements_match() {
                let catalog: $catalog = $open();
                let golden = $crate::compile::Golden::new(&catalog).expect("the binding declares a golden catalog");
                $crate::compile::matches_cases(&golden, &$cases()).expect("the compile cases agree");
            }
        }
    };
    (@universal $catalog:ty, $open:path) => {
        #[test]
        fn the_catalog_provides_exactly_what_it_declares() {
            let catalog: $catalog = $open();
            $crate::compile::declaration_matches(&catalog).expect("the declaration matches");
        }
        #[test]
        fn loading_the_same_catalog_twice_keeps_its_digest() {
            let catalog: $catalog = $open();
            $crate::compile::repeats_its_digest(&catalog).expect("the digest is repeatable");
        }
    };
}
