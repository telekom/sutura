//! The descriptor's layout: one cell per way bytes are refused, and the bytes it mints reading back.
//! The tier half - a descriptor read against a real snapshot - is `tests/partition_read.rs`.

use super::super::Pages;
use super::{InvalidDescriptor, PartitionDescriptor};
use crate::adbc::session::SnapshotId;

/// The shape `pg_export_snapshot()` writes, `%08X-%08X-%d`.
const ID: &str = "00000003-0000001B-1";

fn descriptor(pages: Pages) -> PartitionDescriptor {
    PartitionDescriptor {
        snapshot: SnapshotId::parse(ID).expect("an id the server writes is admitted"),
        pages,
    }
}

/// What a parse read: the snapshot id and the page range - the descriptor has no `PartialEq` of its own.
type Parsed = Result<(String, Pages), InvalidDescriptor>;

fn parsed(bytes: &[u8]) -> Parsed {
    PartitionDescriptor::try_from(bytes).map(|read| (String::from(read.snapshot().as_str()), read.pages()))
}

/// A closed `[3, 9)` range under `id`, laid out by hand rather than minted.
fn closed_under(id: &[u8]) -> Vec<u8> {
    let mut bytes = vec![1, u8::try_from(id.len()).expect("a test id fits its length byte")];
    bytes.extend_from_slice(id);
    bytes.extend_from_slice(&[0, 0, 0, 3, 1, 0, 0, 0, 9]);
    bytes
}

#[test]
fn a_descriptor_of_an_unread_version_is_refused() {
    let mut bytes = descriptor((3, Some(9))).to_bytes();
    bytes[0] = 2;
    assert_eq!(parsed(&bytes), Err(InvalidDescriptor::Version { found: 2 }));
}

#[test]
fn every_proper_prefix_of_a_descriptor_is_refused_as_cut_short() {
    for pages in [(3, Some(9)), (9, None)] {
        let bytes = descriptor(pages).to_bytes();
        for kept in 0..bytes.len() {
            assert_eq!(
                parsed(&bytes[..kept]),
                Err(InvalidDescriptor::Truncated),
                "{pages:?} cut to {kept} of {} bytes",
                bytes.len()
            );
        }
    }
}

#[test]
fn a_descriptor_with_bytes_past_its_last_field_is_refused() {
    for pages in [(3, Some(9)), (9, None)] {
        let mut bytes = descriptor(pages).to_bytes();
        bytes.push(0);
        assert_eq!(parsed(&bytes), Err(InvalidDescriptor::Trailing { extra: 1 }), "{pages:?}");
    }
}

#[test]
fn a_descriptor_whose_snapshot_id_could_close_its_quote_is_refused() {
    assert_eq!(parsed(&closed_under(ID.as_bytes())), Ok((String::from(ID), (3, Some(9)))));
    let too_long = [b'a'; 65];
    let refused: [&[u8]; 4] = [b"00000003-0000'01B-1", b"", &[0xFF], &too_long];
    for id in refused {
        assert_eq!(parsed(&closed_under(id)), Err(InvalidDescriptor::Snapshot), "{id:?}");
    }
}

#[test]
fn a_descriptor_whose_range_end_is_neither_open_nor_closed_is_refused() {
    let mut bytes = descriptor((9, None)).to_bytes();
    *bytes.last_mut().expect("a descriptor is never empty") = 2;
    assert_eq!(parsed(&bytes), Err(InvalidDescriptor::End { found: 2 }));
}

#[test]
fn a_descriptor_range_holding_no_page_is_refused() {
    for (from, to) in [(9, 9), (9, 3)] {
        let bytes = descriptor((from, Some(to))).to_bytes();
        assert_eq!(parsed(&bytes), Err(InvalidDescriptor::EmptyRange { from, to }));
    }
}

#[test]
fn a_descriptor_reads_back_from_its_own_bytes() {
    for pages in [(0, Some(1)), (3, Some(9)), (9, None), (u32::MAX - 1, Some(u32::MAX))] {
        assert_eq!(parsed(&descriptor(pages).to_bytes()), Ok((String::from(ID), pages)));
    }
}

#[test]
fn a_descriptor_debug_never_prints_its_snapshot_id() {
    let printed = format!("{:?}", descriptor((3, Some(9))));
    assert!(printed.contains("from_page: 3"), "{printed}");
    assert!(!printed.contains("0000001B"), "{printed}");
}
