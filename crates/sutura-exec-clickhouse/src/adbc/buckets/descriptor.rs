//! One bucket of a [`super::BucketSplit`] as bytes another node can hand back to
//! [`AdbcClickHouseWarehouse::read_partition`](super::super::AdbcClickHouseWarehouse::read_partition):
//! the bucket's index and count, never the statement - the reading side regenerates that from the
//! leg it already holds.
//!
//! Version 1, in order: the version byte `1`; the bucket count as four big-endian bytes; the
//! bucket index as four big-endian bytes. Every field's width is fixed, so every proper prefix of a
//! descriptor is refused as cut short - dropping the closing count cannot turn one bucket into
//! another.
//!
//! **The limits**: nothing here authenticates the bytes, so a well-formed descriptor nobody minted
//! parses. Their integrity on the wire is the task grant's - `github.com/telekom/sutura#828` - and
//! what a forged one can reach is stated on `read_partition`.

use sutura_sql::generate::{Bucket, InvalidBucket};

const VERSION: u8 = 1;

/// One hash bucket of a leg's split, minted by [`super::BucketSplit`] and parsed from bytes by
/// `TryFrom<&[u8]>`.
///
/// There is no descriptor for a bucket out of range: [`Bucket::new`] refuses an index at or past
/// the count before it is ever encoded.
pub struct BucketDescriptor {
    bucket: Bucket,
}

/// Why bytes are not a [`BucketDescriptor`]. No variant carries the bytes themselves.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidBucketDescriptor {
    /// A version this build does not read.
    #[error("a bucket descriptor of version {found}, where this build reads version 1")]
    Version { found: u8 },
    /// The bytes end before the field they declare does.
    #[error("a bucket descriptor cut short")]
    Truncated,
    /// Bytes past the last field.
    #[error("a bucket descriptor with {extra} bytes past its last field")]
    Trailing { extra: usize },
    /// An index at or past the count, or fewer than two buckets.
    #[error(transparent)]
    Bucket(InvalidBucket),
}

impl BucketDescriptor {
    /// The bucket this descriptor names.
    #[must_use]
    pub const fn bucket(&self) -> Bucket {
        self.bucket
    }

    /// The bytes `TryFrom<&[u8]>` parses back into this descriptor.
    #[must_use]
    #[expect(
        clippy::big_endian_bytes,
        reason = "a descriptor crosses hosts, so its bucket index and count are in network byte order"
    )]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(9);
        bytes.push(VERSION);
        bytes.extend_from_slice(&self.bucket.count().to_be_bytes());
        bytes.extend_from_slice(&self.bucket.index().to_be_bytes());
        bytes
    }
}

impl TryFrom<&[u8]> for BucketDescriptor {
    type Error = InvalidBucketDescriptor;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let (&version, rest) = bytes.split_first().ok_or(InvalidBucketDescriptor::Truncated)?;
        if version != VERSION {
            return Err(InvalidBucketDescriptor::Version { found: version });
        }
        let (count_bytes, rest) = rest.split_at_checked(4).ok_or(InvalidBucketDescriptor::Truncated)?;
        let count = four(count_bytes);
        let (index_bytes, rest) = rest.split_at_checked(4).ok_or(InvalidBucketDescriptor::Truncated)?;
        let index = four(index_bytes);
        if !rest.is_empty() {
            return Err(InvalidBucketDescriptor::Trailing { extra: rest.len() });
        }
        Ok(Self {
            bucket: Bucket::new(index, count).map_err(InvalidBucketDescriptor::Bucket)?,
        })
    }
}

/// Four bytes as one big-endian `u32`; the caller split them out already.
#[expect(
    clippy::big_endian_bytes,
    reason = "a descriptor crosses hosts, so its bucket index and count are in network byte order"
)]
fn four(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap_or([0; 4]))
}

#[cfg(test)]
mod tests {
    use super::{Bucket, BucketDescriptor, InvalidBucketDescriptor};

    #[test]
    fn a_clickhouse_bucket_descriptor_round_trips_and_refuses_malformed_bytes() {
        for (index, count) in [(0, 2), (1, 3), (u32::MAX - 1, u32::MAX)] {
            let bucket = Bucket::new(index, count).expect("a valid bucket");
            let bytes = BucketDescriptor { bucket }.to_bytes();
            let parsed = BucketDescriptor::try_from(bytes.as_slice()).expect("a descriptor reads back");
            assert_eq!(parsed.bucket(), bucket);
            for kept in 0..bytes.len() {
                assert!(matches!(
                    BucketDescriptor::try_from(&bytes[..kept]),
                    Err(InvalidBucketDescriptor::Truncated)
                ));
            }
        }
        for (bytes, wanted) in [
            (
                [2, 0, 0, 0, 3, 0, 0, 0, 1].as_slice(),
                InvalidBucketDescriptor::Version { found: 2 },
            ),
            (
                [1, 0, 0, 0, 3, 0, 0, 0, 1, 0].as_slice(),
                InvalidBucketDescriptor::Trailing { extra: 1 },
            ),
            (
                [1, 0, 0, 0, 1, 0, 0, 0, 0].as_slice(),
                InvalidBucketDescriptor::Bucket(sutura_sql::generate::InvalidBucket::TooFew { count: 1 }),
            ),
            (
                [1, 0, 0, 0, 3, 0, 0, 0, 3].as_slice(),
                InvalidBucketDescriptor::Bucket(sutura_sql::generate::InvalidBucket::PastTheLast { index: 3, count: 3 }),
            ),
        ] {
            assert_eq!(BucketDescriptor::try_from(bytes).err(), Some(wanted));
        }
    }
}
