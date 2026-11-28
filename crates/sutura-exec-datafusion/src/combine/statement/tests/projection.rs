//! The backstop lives apart from `checked`'s own implementation, in a file the implementation
//! change never touches - sharing one file with the change it pins gives a test a non-verdict pass
//! either way, since no base run exists there to redden it.

use super::super::projection::Guarded;
use super::*;

#[derive(Default)]
struct Legs {
    opened: usize,
}

impl StatementLegs for Legs {
    type Stop = ();

    fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), ()> {
        self.opened += 1;
        Ok(())
    }

    fn run(&mut self, _: &SourceName, _: &RawStatement) -> Result<RawRows, ()> {
        Ok(RawRows::of(Vec::new(), Vec::new()))
    }
}

/// A leg statement that still carries sutura's internal collision marker (`\u{1}`) is refused
/// before any source opens - `checked`'s own backstop, the last line before a source's own SQL text
/// would ever see sutura's internal registration separator.
#[test]
fn a_leg_statement_carrying_the_internal_collision_marker_is_refused_before_it_opens() {
    let sources = BTreeSet::from([named("source")]);
    let mangled = RawStatement::parse("SELECT id FROM \"source\u{1}items\"").unwrap();
    let mut legs = Legs::default();
    let result = Guarded::new(&mut legs).open(&sources, [(&mangled, TypeDialect::DataFusion)]);
    assert!(matches!(result, Err(StatementStopped::NotPlanned)), "{result:?}");
    assert_eq!(legs.opened, 0);
}
