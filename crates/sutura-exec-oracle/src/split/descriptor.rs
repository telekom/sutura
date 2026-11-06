//! One partition of an Oracle lookup leg as bytes another node can hand back to
//! [`OracleWarehouse::read_partition`](crate::OracleWarehouse::read_partition): the snapshot and the
//! bucket, never the statement - the reading side renders that from the leg it already holds.
//!
//! Version 1, in order: the version byte `1`; the system change number as eight big-endian bytes;
//! the hashed key's position among the leg's keys as two; the bucket's index and the bucket count as
//! four each. Every field has a fixed width, so every proper prefix is refused as cut short.
//!
//! **The limit:** nothing here authenticates the bytes, so a well-formed descriptor nobody minted
//! parses. Their integrity on the wire is the task grant's (`github.com/telekom/sutura#828`).

use sutura_sql::{InvalidKeyBucket, KeyBucket, Scn};

const VERSION: u8 = 1;

/// A partition's snapshot and bucket, minted by [`super::SplitLeg::partitions`] and parsed from
/// bytes by `TryFrom<&[u8]>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionDescriptor {
    scn: Scn,
    bucket: KeyBucket,
}

/// Why bytes are not a [`PartitionDescriptor`]. No variant carries the bytes themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDescriptor {
    /// A version this build does not read.
    #[error("a partition descriptor of version {found}, which this build does not read")]
    Version { found: u8 },
    /// The bytes end before the field they declare does.
    #[error("a partition descriptor cut short")]
    Truncated,
    /// Bytes past the last field.
    #[error("a partition descriptor with {extra} bytes past its last field")]
    Trailing { extra: usize },
    /// A system change number of zero, at which no snapshot is taken.
    #[error("a partition descriptor whose snapshot is zero")]
    NoSnapshot,
    /// A bucket that is not one of two or more, below the count.
    #[error(transparent)]
    Bucket(#[from] InvalidKeyBucket),
}

impl PartitionDescriptor {
    /// The partition of `bucket` at `scn`.
    #[must_use]
    pub const fn new(scn: Scn, bucket: KeyBucket) -> Self {
        Self { scn, bucket }
    }

    /// The snapshot every table of the partition is read at.
    #[must_use]
    pub const fn scn(&self) -> Scn {
        self.scn
    }

    /// Which bucket of which key the partition keeps.
    #[must_use]
    pub const fn bucket(&self) -> KeyBucket {
        self.bucket
    }

    /// The bytes `TryFrom<&[u8]>` parses back into this descriptor.
    #[must_use]
    #[expect(
        clippy::big_endian_bytes,
        reason = "a descriptor crosses hosts, so its numbers are in network byte order"
    )]
    pub fn to_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(19);
        bytes.push(VERSION);
        bytes.extend_from_slice(&self.scn.get().to_be_bytes());
        bytes.extend_from_slice(&self.bucket.key().to_be_bytes());
        bytes.extend_from_slice(&self.bucket.index().to_be_bytes());
        bytes.extend_from_slice(&self.bucket.count().to_be_bytes());
        bytes
    }
}

impl TryFrom<&[u8]> for PartitionDescriptor {
    type Error = InvalidDescriptor;

    /// **Not built yet:** no version is read, so every descriptor is refused naming its first byte.
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Err(InvalidDescriptor::Version {
            found: bytes.first().copied().unwrap_or(0),
        })
    }
}

#[cfg(test)]
mod tests;
