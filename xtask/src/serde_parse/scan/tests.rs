use super::{
    Shape, code_lines, constructor_name, derives, fallible_constructors, matching_angle, serde_arg, shape_of, string_literals,
};

#[test]
fn a_trait_impl_is_not_an_inherent_impl() {
    // `impl TryFrom<String> for Digest` declares `try_from`, which returns `Result<Self` -
    // and reading it as the type's own constructor would make every correctly-routed newtype
    // a violation, which is the shape of a gate people learn to disable.
    let code = code_lines(
        "impl TryFrom<String> for Digest {\n    fn try_from(raw: String) -> Result<Self, Bad> {\n        Self::parse(raw)\n    }\n}\n",
    );
    assert!(fallible_constructors(&code).is_empty());
}

#[test]
fn a_wrapped_constructor_signature_is_still_read() {
    let code = code_lines(
        "impl Digest {\n    pub fn parse(\n        raw: &str,\n    ) -> Result<Self, Bad> {\n        todo!()\n    }\n}\n",
    );
    assert_eq!(fallible_constructors(&code).get("Digest").map(String::as_str), Some("parse"));
}

#[test]
fn a_generic_impl_names_the_type_after_its_parameters() {
    let code = code_lines("impl<T> Holder<T> {\n    pub fn parse(raw: T) -> Result<Self, Bad> {\n        todo!()\n    }\n}\n");
    assert!(fallible_constructors(&code).contains_key("Holder"));
}

#[test]
fn a_constructor_in_a_later_impl_block_is_not_attributed_to_an_earlier_type() {
    let code = code_lines(
        "impl Anchor {\n    pub const fn new(v: u8) -> Self {\n        Self { v }\n    }\n}\n\nimpl Digest {\n    pub fn parse(raw: &str) -> Result<Self, Bad> {\n        todo!()\n    }\n}\n",
    );
    let found = fallible_constructors(&code);
    assert!(!found.contains_key("Anchor"), "{found:?}");
    assert!(found.contains_key("Digest"), "{found:?}");
}

#[test]
fn a_method_taking_self_is_not_a_constructor() {
    assert!(constructor_name("    pub fn narrowed(&self, to: u8) -> Result<Self, Bad> ").is_none());
}

#[test]
fn a_result_of_another_type_is_not_this_ones_constructor() {
    assert!(constructor_name("    pub fn digest_of(x: &X) -> Result<Digest, Bad> ").is_none());
}

#[test]
fn derive_is_required_before_a_trait_name_counts() {
    assert!(derives("#[derive(Debug, serde::Deserialize)]", "Deserialize"));
    assert!(!derives("#[serde(with = \"Deserialize\")]", "Deserialize"));
    // The suffix comparison earns its keep here: a substring test would read `Deserialize`
    // as a `Serialize` derive and make every deserializing type look symmetric.
    assert!(!derives("#[derive(serde::Deserialize)]", "Serialize"));
}

#[test]
fn a_serde_argument_is_read_by_name() {
    let attrs = "#[serde(try_from = \"TermRepr\", into = \"TermRepr\")]";
    assert_eq!(serde_arg(attrs, "try_from").as_deref(), Some("TermRepr"));
    assert_eq!(serde_arg(attrs, "into").as_deref(), Some("TermRepr"));
    assert!(serde_arg(attrs, "from").is_none());
}

#[test]
fn a_shape_is_read_off_the_declaration() {
    let one = code_lines("pub struct Digest(String);\n");
    assert_eq!(shape_of(&one, 0), Shape::Newtype(String::from("String")));
    let two = code_lines("pub struct Pair(A, B);\n");
    assert_eq!(shape_of(&two, 0), Shape::Other);
    let unit = code_lines("pub struct Marker;\n");
    assert_eq!(shape_of(&unit, 0), Shape::Other);
    let named = code_lines("pub struct Range {\n    start: Date,\n    end: Date,\n}\n");
    assert_eq!(
        shape_of(&named, 0),
        Shape::Named(vec![String::from("start"), String::from("end")])
    );
}

#[test]
fn a_generic_field_type_does_not_split_a_newtype_into_two_fields() {
    let code = code_lines("pub struct Held(BTreeMap<String, Note>);\n");
    assert_eq!(shape_of(&code, 0), Shape::Newtype(String::from("BTreeMap<String, Note>")));
}

#[test]
fn a_body_opened_on_a_later_line_is_still_read() {
    let code = code_lines("pub struct Wrapper<T>\nwhere\n    T: Clone,\n{\n    inner: T,\n}\n");
    assert_eq!(shape_of(&code, 0), Shape::Named(vec![String::from("inner")]));
}

#[test]
fn a_literal_broken_across_lines_is_one_sentence_again() {
    // What `code_lines` BLANKS, this keeps - the same walk, the other question. A `\` at end of
    // line eats the newline and the next line's indentation, so a phrase spanning the break is
    // contiguous; without that rule "with the feature" is "with the" and "feature".
    let text =
        "fn f() {\n    Err(format!(\n        \"build the binary \\\n         with the feature that provides it\"\n    ))\n}\n";
    let found = string_literals(text);
    assert_eq!(found.len(), 1, "one literal");
    let one = found.first().expect("one literal");
    assert_eq!(one.body, "build the binary with the feature that provides it");
    // The line a reader opens is the one the quote OPENS on, not the one it closes on.
    assert_eq!(one.line, 3);
    // And `code_lines` is unchanged by any of it: a multi-line literal is still blanked there.
    assert!(!code_lines(text).join("\n").contains("with the feature"));
}

#[test]
fn a_comment_holds_no_literal_and_a_raw_string_keeps_its_backslashes() {
    // The comment half is what makes this usable as a gate input: prose DESCRIBING a message is
    // not a message. Measured rather than assumed, because sharing `code_lines`'s walk instead
    // of scanning text is the whole reason this reader lives here.
    assert!(string_literals("// a comment saying \"quoted\" things\n").is_empty());
    // In a raw string a backslash is a character, so the continuation rule must not touch it.
    let raw = string_literals("let p = r\"a\\\nb\";\n");
    assert_eq!(raw.first().expect("one literal").body, "a\\\nb");
}

#[test]
fn a_single_line_string_keeps_its_content_because_the_attribute_needs_it() {
    let lines = code_lines("#[serde(try_from = \"String\")]\n");
    assert_eq!(lines.first().map(String::as_str), Some("#[serde(try_from = \"String\")]"));
}

#[test]
fn blanking_a_string_does_not_move_a_line_number() {
    let text = "fn a() {}\nconst X: &str = r#\"\none\ntwo\n\"#;\npub struct Late(String);\n";
    let lines = code_lines(text);
    assert_eq!(
        lines.iter().position(|line| line.contains("struct Late")),
        Some(5),
        "{lines:?}"
    );
}

#[test]
fn a_quote_inside_a_char_literal_opens_no_string() {
    let text = "fn a() { let q = '\"'; }\npub struct After(String);\n";
    let lines = code_lines(text);
    assert!(
        lines.iter().any(|line| line.contains("struct After")),
        "the char literal swallowed the file: {lines:?}"
    );
}

#[test]
fn a_lifetime_is_not_a_char_literal() {
    let text = "impl<'a> Digest<'a> {}\npub struct After(String);\n";
    let lines = code_lines(text);
    assert!(lines.iter().any(|line| line.contains("struct After")), "{lines:?}");
}

#[test]
fn an_arrow_inside_a_generic_list_is_not_its_closing_bracket() {
    // `impl<F: for<'a> Fn(&'a str) -> u8> Holder<F>` closed the parameter list at the arrow,
    // four characters early, so the type after it was unreadable. `crate::newtype_leaks`
    // found it; the guard lives here because both gates read an `impl` header this way.
    let bound = "<F: for<'a> Fn(&'a str) -> u8> Holder<F>";
    let at = matching_angle(bound).expect("the parameter list closes");
    // Asserted by what FOLLOWS rather than by an offset, so the test says the property
    // instead of a number: everything past the bracket is the type being implemented for.
    assert_eq!(bound.get(at..), Some("> Holder<F>"), "closed early at {at}");
    assert_eq!(matching_angle("<T> Holder<T>"), Some(2));
}

#[test]
fn a_generic_impl_with_a_function_bound_still_names_its_type() {
    let code = code_lines(
        "impl<F: for<'a> Fn(&'a str) -> u8> Holder<F> {\n    pub fn parse(raw: F) -> Result<Self, Bad> {\n        todo!()\n    }\n}\n",
    );
    assert!(
        fallible_constructors(&code).contains_key("Holder"),
        "{:?}",
        fallible_constructors(&code)
    );
}

#[test]
fn a_block_comment_hides_a_declaration_and_keeps_the_lines() {
    let text = "/*\npub struct Hidden(String);\n*/\npub struct Real(String);\n";
    let lines = code_lines(text);
    assert!(!lines.iter().any(|line| line.contains("struct Hidden")), "{lines:?}");
    assert_eq!(lines.iter().position(|line| line.contains("struct Real")), Some(3));
}
