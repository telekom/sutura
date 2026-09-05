//! Tests for the parsed strings in [`super`], one per refusal and one per bound.
//!
//! A file rather than an inline `mod tests`, for the reason [`super`] is a file: the module plus
//! these cases is over the thousand-line limit `cargo xtask max-lines` enforces.
//!
//! The character-set tests walk **both ends of every range** and assert that the code point on each
//! side is NOT refused, which is [`crate::text`]'s own test style. That is the check a sampled test
//! cannot have: a typo in a range bound is invisible to a test that only tries `U+200B`.

use super::{
    AnchorValue, Description, DimensionValue, InvalidDescription, InvalidDimensionValue, MAX_DESCRIPTION_BYTES,
    MAX_DESCRIPTION_LINES, MAX_DIMENSION_VALUE_CHARS,
};

/// Both ends of all seven ranges [`crate::text::is_invisible`] names.
const INVISIBLE: &[u32] = &[
    0x00AD, 0x200B, 0x200F, 0x202A, 0x202E, 0x2060, 0x2064, 0x2066, 0x2069, 0xFEFF, 0xFFF9, 0xFFFB,
];

/// The code point on each side of each of those ranges. None of them is refused as invisible, so
/// this is the set written down rather than a wider sweep that would reject ordinary text.
const NEIGHBOURS: &[u32] = &[
    0x00AC, 0x00AE, 0x200A, 0x2010, 0x2029, 0x202F, 0x205F, 0x2065, 0x206A, 0xFEFE, 0xFF00, 0xFFFC,
];

fn character(code: u32) -> char {
    char::from_u32(code).expect("a listed code point is a character")
}

// ---------------------------------------------------------------- DimensionValue ----

#[test]
fn an_ordinary_value_parses_and_is_stored_exactly_as_written() {
    // Normalises nothing: not the case, not the internal space, not the punctuation. A value is
    // compared against what a data system holds, so any folding here would be a comparison against
    // something the author did not write.
    for raw in [
        "north",
        "fixed_internet",
        "Tariff L",
        "GAP-2026/Q1",
        "\u{dc}bergang",
        "\u{5317}",
    ] {
        let parsed = DimensionValue::parse(raw).expect("an ordinary value is a value");
        assert_eq!(parsed.as_str(), raw);
        assert_eq!(parsed.to_string(), raw);
    }
}

#[test]
fn nothing_is_not_a_value() {
    assert_eq!(DimensionValue::parse(""), Err(InvalidDimensionValue::Empty));
}

#[test]
fn a_value_is_one_line_and_carries_no_control_character() {
    // A newline is the one that matters: the prompt renders declared values inline in a
    // comma-separated list, so a newline in one writes a line of that document.
    for raw in ["north\nsouth", "nor\tth", "north\u{0007}", "\rnorth"] {
        assert_eq!(
            DimensionValue::parse(raw),
            Err(InvalidDimensionValue::ControlCharacter {
                value: String::from(raw)
            }),
            "{raw:?}"
        );
    }
}

#[test]
fn every_invisible_range_is_refused_at_both_ends_and_no_neighbour_of_one_is() {
    for &code in INVISIBLE {
        let offending = character(code);
        let raw = format!("nor{offending}th");
        assert_eq!(
            DimensionValue::parse(&raw),
            Err(InvalidDimensionValue::InvisibleCharacter {
                value: raw.clone(),
                code
            }),
            "{code:#06x}"
        );
        // The premise the whole refusal rests on: general category `Cf`, so the control-character
        // check above sees none of these and this is a second refusal rather than a wider one.
        assert!(!offending.is_control(), "{code:#06x} is not a control character");
    }
    for &code in NEIGHBOURS {
        let benign = character(code);
        let raw = format!("nor{benign}th");
        // `U+2029` is a paragraph separator and `U+202F` a narrow no-break space, so two of the
        // neighbours are refused for a DIFFERENT reason - which is the point: none of them is
        // refused as invisible.
        let outcome = DimensionValue::parse(&raw);
        assert!(
            !matches!(outcome, Err(InvalidDimensionValue::InvisibleCharacter { .. })),
            "{code:#06x} is not one of the invisible ones: {outcome:?}"
        );
    }
}

#[test]
fn spacing_a_reader_cannot_account_for_is_refused() {
    for raw in [
        " north",            // leading
        "north ",            // trailing
        "north  east",       // doubled
        "north\u{00A0}east", // a no-break space, which draws as one and is not one
        "north\u{3000}east", // an ideographic space, likewise
        " ",                 // nothing but spacing
    ] {
        assert_eq!(
            DimensionValue::parse(raw),
            Err(InvalidDimensionValue::Spacing {
                value: String::from(raw)
            }),
            "{raw:?}"
        );
    }
    // One plain space between words is spacing a reader can see, and it stays legal: a product name
    // and a region name have one.
    drop(DimensionValue::parse("north east").expect("one plain space between words is spacing a reader sees"));
    drop(DimensionValue::parse("Tariff L Business").expect("a product name has spaces in it"));
}

#[test]
fn the_length_bound_is_checked_at_both_ends() {
    let at_limit = "v".repeat(MAX_DIMENSION_VALUE_CHARS);
    assert_eq!(
        DimensionValue::parse(&at_limit)
            .expect("a value at the limit is a value")
            .as_str(),
        at_limit
    );
    let over = "v".repeat(MAX_DIMENSION_VALUE_CHARS.saturating_add(1));
    assert_eq!(
        DimensionValue::parse(&over),
        Err(InvalidDimensionValue::TooLong {
            value: over,
            len: MAX_DIMENSION_VALUE_CHARS.saturating_add(1),
            limit: MAX_DIMENSION_VALUE_CHARS,
        })
    );
    // Characters and not bytes, so a value legal in English is legal in German: 64 two-byte letters
    // are 128 bytes and 64 characters.
    let umlauts = "\u{e4}".repeat(MAX_DIMENSION_VALUE_CHARS);
    assert_eq!(umlauts.len(), MAX_DIMENSION_VALUE_CHARS.saturating_mul(2));
    drop(DimensionValue::parse(&umlauts).expect("64 two-byte letters are 64 characters"));
}

#[test]
fn the_deserialization_path_is_the_one_constructor() {
    // `#[serde(try_from = "String")]` names this impl, so a catalog document's `values:` list goes
    // through `parse` rather than straight into the private field. Asserted through `TryFrom`
    // because the on-disk format belongs to `sutura-catalog-local`, which has
    // a real format parser and asserts the same thing over YAML.
    assert_eq!(
        DimensionValue::try_from(String::from("north"))
            .expect("a value is a value")
            .as_str(),
        "north"
    );
    assert_eq!(
        DimensionValue::try_from(String::from("nor\u{200B}th")),
        Err(InvalidDimensionValue::InvisibleCharacter {
            value: String::from("nor\u{200B}th"),
            code: 0x200B,
        })
    );
}

// ------------------------------------------------------------------ AnchorValue ----

/// The rule is shared by calling it, so the anchor value refuses what a declared value refuses.
///
/// One case per fault rather than a copy of every range sweep above: what is asserted here is that
/// `authored_scalar` is reached, not the rule's own edges - those have their own tests, over the one
/// implementation. A second sweep would be a second thing to keep true, which is what the shared
/// function exists to prevent.
#[test]
fn an_anchor_value_is_held_to_the_declared_values_rule() {
    assert_eq!(
        AnchorValue::parse("197122").expect("a rendered number is a value").as_str(),
        "197122"
    );
    assert_eq!(AnchorValue::parse(""), Err(InvalidDimensionValue::Empty));
    assert_eq!(
        AnchorValue::parse("1971\u{200F}22"),
        Err(InvalidDimensionValue::InvisibleCharacter {
            value: String::from("1971\u{200F}22"),
            code: 0x200F,
        })
    );
    assert_eq!(
        AnchorValue::parse("197122\n"),
        Err(InvalidDimensionValue::ControlCharacter {
            value: String::from("197122\n"),
        })
    );
    assert_eq!(
        AnchorValue::parse(" 197122"),
        Err(InvalidDimensionValue::Spacing {
            value: String::from(" 197122"),
        })
    );
}

/// The cap is the declared value's cap, not one of this type's own.
///
/// Both ends, and the lower end is what the assertion is for: an anchor is a rendered number and the
/// longest `i64` is 20 characters, so a cap that refused anything a data system can return as a
/// scalar would be a bound that breaks a legitimate catalog.
#[test]
fn an_anchor_values_length_bound_is_the_declared_values() {
    let longest_i64 = i64::MIN.to_string();
    assert_eq!(longest_i64.len(), 20);
    drop(AnchorValue::parse(&longest_i64).expect("the longest i64 renders well inside the cap"));

    let at = "9".repeat(MAX_DIMENSION_VALUE_CHARS);
    drop(AnchorValue::parse(&at).expect("the cap itself is legal"));
    let over = "9".repeat(MAX_DIMENSION_VALUE_CHARS.saturating_add(1));
    assert_eq!(
        AnchorValue::parse(&over),
        Err(InvalidDimensionValue::TooLong {
            value: over,
            len: MAX_DIMENSION_VALUE_CHARS.saturating_add(1),
            limit: MAX_DIMENSION_VALUE_CHARS,
        })
    );
}

/// A document's `value:` goes through `parse`, not into the private field.
///
/// `sutura_catalog_datahub`'s `SuturaAnchor` deserializes this type, so the `try_from` is the route
/// a deployment-defined property's anchor value actually takes. The markdown adapter does NOT -
/// its `AnchorLiteral::Text` stays a `String` and parses one step later, because `untagged` erases
/// the cause - so this case covers one of the two adapters and the other one's is in its own file.
/// Same assertion, and same reason, as the declared value's above.
#[test]
fn an_anchor_values_deserialization_path_is_the_one_constructor() {
    assert_eq!(
        AnchorValue::try_from(String::from("197122"))
            .expect("a rendered number is a value")
            .as_str(),
        "197122"
    );
    assert_eq!(
        AnchorValue::try_from(String::from("197\u{00A0}122")),
        Err(InvalidDimensionValue::Spacing {
            value: String::from("197\u{00A0}122"),
        })
    );
}

// ------------------------------------------------------------------- Description ----

#[test]
fn a_description_is_prose_and_keeps_its_own_line_breaks() {
    let raw = "Net revenue.\n\nPer CUSTOMER and not per subscription, which is why this is a ratio.";
    let parsed = Description::parse(raw).expect("prose is a description");
    assert_eq!(parsed.as_str(), raw);
    assert!(!parsed.is_empty());
}

#[test]
fn no_prose_at_all_is_a_description_and_the_surrounding_whitespace_goes() {
    // A definition document with nothing under its frontmatter arrives here as the newline that
    // followed the fence. Empty is legal - `sutura_app::prompt` renders no quoted block for one and
    // `sutura-cli` prints no paragraph - and this is what makes the two agree on what "empty" is.
    for raw in ["", "\n", "   \n\t\n"] {
        let parsed = Description::parse(raw).expect("no prose is a description");
        assert!(parsed.is_empty(), "{raw:?}");
        assert_eq!(parsed.as_str(), "");
    }
    assert!(Description::default().is_empty());
    assert_eq!(Description::default(), Description::parse("").expect("empty parses"));
    assert_eq!(
        Description::parse("\n  Net revenue.\n")
            .expect("prose is a description")
            .as_str(),
        "Net revenue."
    );
}

#[test]
fn an_invisible_character_in_prose_is_refused_at_both_ends_of_every_range() {
    // THE ONE THIS TYPE EXISTS FOR. A description mixing one of these into a sentence renders as a
    // paragraph that reads correctly and is not what it says, and no length, emptiness or
    // control-character check can see it.
    for &code in INVISIBLE {
        let offending = character(code);
        let raw = format!("Revenue from status = 'act{offending}ive' subscriptions.");
        assert_eq!(
            Description::parse(&raw),
            Err(InvalidDescription::InvisibleCharacter { code }),
            "{code:#06x}"
        );
    }
    for &code in NEIGHBOURS {
        let benign = character(code);
        let raw = format!("Revenue from act{benign}ive subscriptions.");
        assert!(
            Description::parse(&raw).is_ok(),
            "{code:#06x} is not one of the invisible ones"
        );
    }
}

#[test]
fn a_control_character_the_renderer_would_drop_is_refused_and_the_two_it_keeps_are_not() {
    // The other half of *refuse at load, never alter at render*, and the half this type did not
    // have. `sutura_app::prompt::quote` keeps `\n` and `\t` and drops every other control character,
    // so a description carrying one rendered into the prompt as something other than what the file
    // says - the alteration at render the rule forbids, with nothing downstream able to tell.
    for code in [0x00_u32, 0x07, 0x0B, 0x0C, 0x1B, 0x7F] {
        let offending = character(code);
        let raw = format!("Revenue from status = 'act{offending}ive' subscriptions.");
        assert_eq!(
            Description::parse(&raw),
            Err(InvalidDescription::ControlCharacter { code }),
            "{code:#06x}"
        );
    }
    // The reachable one, and the reason this is not a theoretical case: a CRLF working tree gives
    // every line of every description a trailing `\r`. `frontmatter` strips one at the fence lines
    // alone and `cargo xtask line-endings` sees tracked files only, so a catalog directory mounted
    // from a Windows editor arrives here like this.
    assert_eq!(
        Description::parse("Net revenue.\r\n\r\nPer customer.\r\n"),
        Err(InvalidDescription::ControlCharacter { code: 0x0D })
    );
    // And the two the renderer keeps stay content, which is what makes this the renderer's set
    // rather than every control character.
    drop(Description::parse("Net revenue.\n\n\tPer customer.").expect("a newline and a tab are prose"));
}

#[test]
fn a_control_character_is_reported_before_an_invisible_one() {
    // The order the checks run in, asserted rather than left to the reading: a description holding
    // both is reported as the control character, because "your editor wrote CRLF" is the more
    // useful thing to say than "there is an exotic code point in here somewhere".
    assert_eq!(
        // The `\r` is inside the prose rather than at the end of it, because `parse` trims first and
        // a trailing CRLF is exactly what a document body arrives with.
        Description::parse("Net\u{202E}revenue.\r\nPer customer."),
        Err(InvalidDescription::ControlCharacter { code: 0x0D })
    );
}

#[test]
fn the_byte_cap_is_checked_at_both_ends() {
    let at_limit = "p".repeat(MAX_DESCRIPTION_BYTES);
    assert_eq!(
        Description::parse(&at_limit)
            .expect("prose at the limit is a description")
            .as_str()
            .len(),
        MAX_DESCRIPTION_BYTES
    );
    let over = "p".repeat(MAX_DESCRIPTION_BYTES.saturating_add(1));
    assert_eq!(
        Description::parse(&over),
        Err(InvalidDescription::TooLong {
            len: MAX_DESCRIPTION_BYTES.saturating_add(1),
            limit: MAX_DESCRIPTION_BYTES,
        })
    );
}

#[test]
fn the_line_cap_is_checked_at_both_ends_and_is_not_the_byte_cap() {
    let at_limit = "p\n".repeat(MAX_DESCRIPTION_LINES);
    drop(Description::parse(&at_limit).expect("prose at the line limit is a description"));
    // One more line, and well inside the byte budget - which is why the two caps are separate.
    let over = "p\n".repeat(MAX_DESCRIPTION_LINES.saturating_add(1));
    assert!(over.len() < MAX_DESCRIPTION_BYTES);
    assert_eq!(
        Description::parse(&over),
        Err(InvalidDescription::TooManyLines {
            len: MAX_DESCRIPTION_LINES.saturating_add(1),
            limit: MAX_DESCRIPTION_LINES,
        })
    );
}

#[test]
fn a_descriptions_deserialization_path_is_the_one_constructor() {
    assert_eq!(
        Description::try_from(String::from("Net revenue."))
            .expect("prose is a description")
            .as_str(),
        "Net revenue."
    );
    assert_eq!(
        Description::try_from(String::from("Net\u{202E}revenue.")),
        Err(InvalidDescription::InvisibleCharacter { code: 0x202E })
    );
}
