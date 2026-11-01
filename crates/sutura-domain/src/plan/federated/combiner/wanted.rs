//! The demand a serving node exports: what a hold adds, a dropped hold takes back, across clones.

use super::WorkerSlotsWanted;

#[test]
fn a_hold_is_counted_by_every_clone_until_it_is_dropped() {
    let wanted = WorkerSlotsWanted::default();
    let exported = wanted.clone();
    let first = wanted.hold(3);
    let second = wanted.hold(2);
    assert_eq!(exported.count(), 5, "a clone reads every live hold");
    drop(first);
    assert_eq!(exported.count(), 2, "a dropped hold takes back exactly its own slots");
    drop(second);
    assert_eq!(exported.count(), 0, "no hold, no demand");
}
