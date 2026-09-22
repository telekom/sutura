//! The MONEY bound on one job: how much scanning a deployment is willing to pay for.
//!
//! Separate from `super::MOST_RESULT_ROWS`, which bounds what this PROCESS materialises, and from
//! the port's `Deadline`, which bounds TIME. A question can satisfy both and still scan a
//! partitioned table end to end, which is what this file exists to stop.

/// The most a single job this transport submits may be billed for scanning.
///
/// **Sent as the pinned driver's `bigquery.query.max_bytes_billed` statement option, which that
/// driver maps onto `BigQuery`'s own `maximumBytesBilled` job configuration - so the bound is
/// enforced at the service and not by a check here.** A job that would exceed it fails and is not
/// charged. That is what makes it worth more than a client-side estimate: nothing on this side has
/// to be consulted, kept accurate, or trusted.
///
/// **A newtype and not the `u64` the settings tree holds, because two values in that range are not
/// ceilings.** Zero is `BigQuery`'s own spelling of *no ceiling* - the field is read as unset below
/// one - so a deployment that wrote `0` asked for a bound and would have been given none; and a
/// value above [`Self::MAX_BYTES`] is indistinguishable from no ceiling in practice. Both are
/// refused by [`Self::parse`], which is the only constructor, at the composition root, before a
/// listener is bound.
///
/// **The limit, and it is the whole of what this bound is not.** `maximumBytesBilled` bounds BYTES
/// BILLED for one job. It is not a bound on a deployment's total spend, on one subject's spend, or
/// on a window - `governance.per_replica_spend_ceiling` is that key and it charges only priced
/// estimates, which this transport does not produce (see [`super::AdbcError::NoDryRun`]). It is
/// also not a bound on a job billed for SLOT TIME rather than bytes scanned: on a
/// capacity-priced reservation the bytes a job scans are not what it costs, and this ceiling then
/// bounds the scan without bounding the bill. And it is per JOB, so N questions cost N times it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytesBilledCeiling {
    /// Held as the `i64` the option carries rather than the `u64` the settings key is written in,
    /// so the send site has no conversion that could fail or saturate: [`Self::parse`] is the one
    /// place the range is decided.
    bytes: i64,
}

/// Why a configured bytes-billed ceiling is not one this transport will send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnusableCeiling {
    /// Zero, which `BigQuery` reads as no ceiling rather than as a ceiling of nothing.
    #[error("a bytes-billed ceiling of zero is how BigQuery spells no ceiling at all, so it cannot be one")]
    Zero,
    /// Above what this transport will send, per [`BytesBilledCeiling::MAX_BYTES`].
    #[error("a bytes-billed ceiling of {given} bytes is above the {cap} this transport will send")]
    TooLarge {
        /// What the deployment wrote.
        given: u64,
        /// The largest ceiling this transport sends.
        cap: u64,
    },
}

impl BytesBilledCeiling {
    /// The largest ceiling this transport will send: one tebibyte.
    ///
    /// Not the endpoint's own limit - it has none - but a bound on the bound. A ceiling above this
    /// is indistinguishable from no ceiling, and *no ceiling* is the state this type exists to make
    /// unrepresentable. It is also comfortably inside the `i64` the driver's integer option carries,
    /// so no value this parse admits can overflow the send.
    pub const MAX_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

    /// Parses a ceiling in bytes, as `sources.<alias>.max_bytes_billed` writes it.
    pub const fn parse(bytes: u64) -> Result<Self, UnusableCeiling> {
        if bytes == 0 {
            return Err(UnusableCeiling::Zero);
        }
        if bytes > Self::MAX_BYTES {
            return Err(UnusableCeiling::TooLarge {
                given: bytes,
                cap: Self::MAX_BYTES,
            });
        }
        #[expect(
            clippy::cast_possible_wrap,
            reason = "bounded by MAX_BYTES one line up, which is 2^40 and inside i64"
        )]
        Ok(Self { bytes: bytes as i64 })
    }

    /// The ceiling as the driver's integer statement option carries it.
    pub(super) const fn as_int(self) -> i64 {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::{BytesBilledCeiling, UnusableCeiling};

    #[test]
    fn a_ceiling_of_zero_is_refused_rather_than_sent_as_no_ceiling() {
        // The defect this type is here for: `max_bytes_billed: 0` reached the settings tree, was
        // read by nothing, and would now be sent as the value BigQuery reads as *no limit* - a
        // deployment that asked for the tightest possible bound getting none at all.
        assert_eq!(BytesBilledCeiling::parse(0), Err(UnusableCeiling::Zero));
    }

    #[test]
    fn a_ceiling_above_what_this_transport_sends_is_refused() {
        let given = BytesBilledCeiling::MAX_BYTES + 1;
        assert_eq!(
            BytesBilledCeiling::parse(given),
            Err(UnusableCeiling::TooLarge {
                given,
                cap: BytesBilledCeiling::MAX_BYTES,
            })
        );
        assert_eq!(
            BytesBilledCeiling::parse(u64::MAX),
            Err(UnusableCeiling::TooLarge {
                given: u64::MAX,
                cap: BytesBilledCeiling::MAX_BYTES,
            }),
            "`u64::MAX` booted green before this parse existed"
        );
    }

    #[test]
    fn a_usable_ceiling_reaches_the_option_as_the_bytes_it_was_written_as() {
        // Not a round trip for its own sake: the option is an `i64` and the settings key a `u64`,
        // so this is the one place the conversion is asserted rather than assumed.
        let ceiling = BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a usable ceiling");
        assert_eq!(ceiling.as_int(), 1024 * 1024 * 1024);
        assert_eq!(
            BytesBilledCeiling::parse(BytesBilledCeiling::MAX_BYTES)
                .expect("the cap itself is usable")
                .as_int(),
            1024 * 1024 * 1024 * 1024,
            "the cap is inclusive and inside i64"
        );
        assert_eq!(
            BytesBilledCeiling::parse(1)
                .expect("one byte is a bound, however useless")
                .as_int(),
            1
        );
    }
}
