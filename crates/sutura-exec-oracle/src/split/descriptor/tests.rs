//! A descriptor's bytes, written out by hand, and every way bytes are refused.

use sutura_sql::{InvalidKeyBucket, KeyBucket, Scn};

use super::{InvalidDescriptor, PartitionDescriptor};

/// Bucket 2 of 3 of the key at position 1, at SCN `0x0102030405060708`, as version 1 lays it out.
const WRITTEN: [u8; 19] = [1, 1, 2, 3, 4, 5, 6, 7, 8, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3];

fn parse(bytes: &[u8]) -> Result<PartitionDescriptor, InvalidDescriptor> {
    PartitionDescriptor::try_from(bytes)
}

/// The layout is the literal above, not what `to_bytes` happens to write: both directions are read
/// against it.
#[test]
fn a_descriptor_is_its_snapshot_and_its_bucket_in_network_byte_order() {
    let parsed = parse(&WRITTEN).expect("version 1 parses");
    assert_eq!(parsed.scn(), Scn::parse(0x0102_0304_0506_0708).expect("not zero"));
    assert_eq!(parsed.bucket(), KeyBucket::new(1, 2, 3).expect("a bucket"));
    assert_eq!(parsed.to_bytes(), WRITTEN);
}

/// **Every proper prefix is cut short, and a byte past the last field is trailing**, so dropping or
/// adding a byte never reads as another descriptor.
#[test]
fn a_descriptor_cut_short_or_with_bytes_past_its_end_is_refused() {
    for end in 0..WRITTEN.len() {
        assert_eq!(parse(&WRITTEN[..end]), Err(InvalidDescriptor::Truncated), "{end} bytes");
    }
    let mut longer = WRITTEN.to_vec();
    longer.extend_from_slice(&[0, 0]);
    assert_eq!(parse(&longer), Err(InvalidDescriptor::Trailing { extra: 2 }));
}

/// Another version, a zero snapshot, fewer than two buckets and an index past the last are each
/// their own refusal.
#[test]
fn a_descriptor_of_another_version_no_snapshot_or_no_bucket_is_refused() {
    let with = |at: usize, byte: u8| {
        let mut bytes = WRITTEN;
        bytes[at] = byte;
        parse(&bytes)
    };
    assert_eq!(with(0, 2), Err(InvalidDescriptor::Version { found: 2 }));

    let mut zero = WRITTEN;
    zero[1..9].fill(0);
    assert_eq!(parse(&zero), Err(InvalidDescriptor::NoSnapshot));

    assert_eq!(
        with(18, 1),
        Err(InvalidDescriptor::Bucket(InvalidKeyBucket::TooFew { count: 1 }))
    );
    assert_eq!(
        with(14, 3),
        Err(InvalidDescriptor::Bucket(InvalidKeyBucket::PastTheLast {
            index: 3,
            count: 3
        }))
    );
}
