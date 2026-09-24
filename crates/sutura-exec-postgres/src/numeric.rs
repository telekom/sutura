//! The Postgres `NUMERIC` wire format, decoded exactly.
//!
//! **Its own module because it is one subject and because `lib.rs` is at the unexemptable
//! `max-lines` cap**, split along the seam the file already had rather than wherever the counter
//! fell: everything here turns the documented binary layout of one type into a domain [`Value`],
//! and nothing here knows about a connection, a plan or a credential.
//!
//! `tokio-postgres` at the resolved version ships no `FromSql` for `NUMERIC`, and `sum(int8)` and
//! `AVG` over an integer column return exactly that type - so the decode is ours, and keeping it
//! exact (no `f64` anywhere on the path) is the whole point.

use tokio_postgres::types::{FromSql, Type};

use sutura_domain::warehouse::Value;

use crate::PostgresError;

/// A `NUMERIC`, decoded from the wire as its exact components.
///
/// `tokio-postgres` at the resolved version ships NO `FromSql` for `NUMERIC` (the type OID exists, a Rust type does
/// not), and `sum(int8)` / `AVG` over an integer column return exactly `NUMERIC`. So this is a
/// hand-rolled decoder of the documented binary format - the same decision as `crate::PgDate`: the raw
/// bytes are all the driver gives. It is kept EXACT (there is no `f64` on the value), then mapped
/// through the same shared decimal boundary as the other adapters.
///
/// A non-finite value (`NaN`, `±Infinity`) is carried by its sign word alone
/// ([`PgNumeric::is_not_finite`]) so the caller refuses it at the same place every other non-finite
/// cell is refused, rather than as a driver error.
#[derive(Debug, Clone)]
pub(crate) struct PgNumeric {
    /// The base-10000 digits, most significant first.
    digits: Vec<u16>,
    /// The exponent of `10000` for the first digit.
    weight: i16,
    /// The sign word.
    sign: u16,
    /// The display scale: how many decimal digits the server declares to the right of the point.
    dscale: u16,
}

impl PgNumeric {
    const NEGATIVE: u16 = 0x4000;
    const NAN: u16 = 0xC000;
    const POSITIVE_INFINITY: u16 = 0xD000;
    const NEGATIVE_INFINITY: u16 = 0xF000;

    /// A `NaN` or `±Infinity` numeric, which has no finite value to carry.
    const fn is_not_finite(&self) -> bool {
        matches!(self.sign, Self::NAN | Self::POSITIVE_INFINITY | Self::NEGATIVE_INFINITY)
    }
}

impl<'a> FromSql<'a> for PgNumeric {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        decode_numeric(raw)
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::NUMERIC
    }
}

/// Maps a decoded `NUMERIC` exactly: a fitting integer is numeric; every other finite value is text.
pub(crate) fn numeric_cell(value: &PgNumeric, label: &str) -> Result<Value, PostgresError> {
    if value.is_not_finite() {
        return Err(PostgresError::NotFinite {
            column: String::from(label),
            cause: sutura_domain::warehouse::NotFinite::NotANumber,
        });
    }
    let text = render_numeric(value);
    if value.dscale == 0 {
        Ok(text.parse::<i64>().map_or_else(|_| Value::Text(text), Value::Integer))
    } else {
        Ok(Value::Text(text))
    }
}

/// The exact decimal text of a finite `NUMERIC`.
///
/// Rendered straight from the base-10000 digits and `weight`, so it introduces no `f64` and no
/// rounding anywhere: a `Numeric` is `Σ digit[i] · 10000^(weight−i)`, and expanding each group to
/// its four decimal places with the point after `weight+1` groups of the integer part is exact. The
/// declared `dscale` decides how many digits sit after the point.
fn render_numeric(value: &PgNumeric) -> String {
    let mut text = String::new();
    if value.sign == PgNumeric::NEGATIVE {
        text.push('-');
    }
    text.push_str(&integer_part(&value.digits, value.weight));
    if value.dscale > 0 {
        text.push('.');
        text.push_str(&fraction_part(&value.digits, value.weight, value.dscale));
    }
    text
}

/// The integer part of a `NUMERIC`: `weight+1` base-10000 groups, the leading one un-padded and the
/// rest to four digits, with missing trailing groups read as zero.
fn integer_part(digits: &[u16], weight: i16) -> String {
    if weight < 0 {
        return String::from("0");
    }
    let mut out = String::new();
    match digits.first() {
        Some(leading) => out.push_str(&leading.to_string()),
        None => out.push('0'),
    }
    let groups = usize::from(u16::try_from(weight).unwrap_or(0));
    for index in 1..=groups {
        match digits.get(index) {
            Some(digit) => {
                for c in format!("{digit:04}").chars() {
                    out.push(c);
                }
            }
            None => out.push_str("0000"),
        }
    }
    out
}

/// The fractional part of a `NUMERIC`, to exactly `dscale` decimal digits.
fn fraction_part(digits: &[u16], weight: i16, dscale: u16) -> String {
    // Fractional group zero is 10^-4. Its source digit is `weight+1`; a negative index is an
    // omitted zero group before the first stored digit, not permission to start at digit zero.
    let base = i32::from(weight) + 1;
    let mut out = String::new();
    let mut gathered: u16 = 0;
    let mut group_index: i32 = 0;
    while gathered < dscale {
        let source_index = usize::try_from(base + group_index).ok();
        let group: Vec<char> = source_index
            .and_then(|index| digits.get(index))
            .map_or_else(|| vec!['0', '0', '0', '0'], |digit| format!("{digit:04}").chars().collect());
        let need = usize::from(dscale - gathered);
        for c in group.iter().take(need) {
            out.push(*c);
        }
        gathered = gathered.saturating_add(u16::try_from(need.min(4)).unwrap_or(0));
        group_index += 1;
    }
    out
}

/// One big-endian `u16` at `offset`, if the bytes are there.
#[expect(
    clippy::big_endian_bytes,
    reason = "the Postgres NUMERIC wire format is documented big-endian, so reading a u16 is a \
              direct big-endian decode rather than an accident"
)]
fn u16_at(raw: &[u8], offset: usize) -> Option<u16> {
    let slice: [u8; 2] = raw.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(slice))
}

/// The error a wire decoder returns: a message-only error, because the driver's raw bytes carry no
/// typed context to preserve.
type WireError = Box<dyn std::error::Error + Sync + Send>;

/// The Postgres `NUMERIC` binary format: two bytes of digit count, two of weight, two of sign, two
/// of display scale, then `ndigits` base-10000 digits. Values combine as
/// `Σ digit[i] · 10000^(weight − i)`.
#[expect(
    clippy::cast_possible_wrap,
    reason = "the wire weight is a signed i16 carried as two bytes, so reinterpreting the unsigned \
              read as i16 is the documented decode, not an arithmetic wrap"
)]
pub(crate) fn decode_numeric(raw: &[u8]) -> Result<PgNumeric, WireError> {
    if raw.len() < 8 {
        return Err("a NUMERIC came back shorter than its header".into());
    }
    let ndigits = u16_at(raw, 0).ok_or("a NUMERIC header was truncated")?;
    let weight_bits = u16_at(raw, 2).ok_or("a NUMERIC header was truncated")?;
    let sign = u16_at(raw, 4).ok_or("a NUMERIC header was truncated")?;
    let dscale = u16_at(raw, 6).ok_or("a NUMERIC header was truncated")?;
    let mut digits = Vec::with_capacity(usize::from(ndigits));
    for index in 0..ndigits {
        digits.push(u16_at(raw, 8 + 2 * usize::from(index)).ok_or("a NUMERIC value was truncated")?);
    }
    Ok(PgNumeric {
        digits,
        weight: weight_bits as i16,
        sign,
        dscale,
    })
}
