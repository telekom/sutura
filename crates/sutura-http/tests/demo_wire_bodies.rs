#![forbid(unsafe_code)]
//! Every `/v1/query` body committed outside this crate - the demo healthcheck, the release smoke
//! test, and the demo's fake server's expectations - parses as the wire shapes this crate actually
//! serves, `github.com/telekom/sutura#1020`.
//!
//! `#968` renamed `QuestionBody`'s `metric` field to `metrics` and made `FilterBody` tagged by
//! `op`, both under `deny_unknown_fields`. Three bodies outside this crate's own tests still spelled
//! the old shape and were caught only by a red release job and a red `demo-container` CI leg, not by
//! `just test` - nothing here read them. This file is the read: it extracts the literal JSON each
//! script sends and feeds it through the same `serde_json::from_str::<QuestionBody>` the server
//! does, so a wire rename that misses one of these fails `just test` instead of a deploy.

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sutura_http::wire::QuestionBody;

    /// The workspace root, from this crate's own manifest directory - `crates/sutura-http/..`
    /// is one hop short, so two hops up.
    fn workspace_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crates/sutura-http sits two directories under the workspace root")
            .to_path_buf()
    }

    fn read(rel: &str) -> String {
        let path = workspace_root().join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// The `{...}` starting right after `marker`, taken by brace counting rather than a fixed
    /// length - so a body that grows a field is still captured whole. Both the Python dict
    /// literals in `demo/healthcheck.py` and the JSON in `.github/serve-smoke.sh`'s `-d '...'`
    /// are plain double-quoted JSON already, with no Python-only syntax, so this is a JSON
    /// extraction and not a Python one.
    fn json_object_after<'a>(source: &'a str, marker: &str) -> &'a str {
        let start = source.find(marker).unwrap_or_else(|| panic!("{marker:?} not found")) + marker.len();
        let tail = source.get(start..).expect("`start` is a char boundary: it follows `marker`");
        let open = tail.find('{').unwrap_or_else(|| panic!("no {{ after {marker:?}")) + start;
        let scope = source
            .get(open..)
            .expect("`open` is a char boundary: it is where `find('{')` landed");
        let mut depth = 0_i32;
        for (offset, ch) in scope.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = open + offset + 1;
                        return source
                            .get(open..end)
                            .expect("both ends sit on a `{`/`}` byte, always a char boundary");
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced braces after {marker:?}")
    }

    /// Python allows a trailing comma before a closing `}`/`]`; strict JSON, and `serde_json`,
    /// does not. The dict literals this file reads out of `demo/healthcheck.py` are otherwise
    /// exactly JSON, so this is the one Python-ism to undo before parsing.
    fn drop_trailing_commas(json: &str) -> String {
        let mut out = String::with_capacity(json.len());
        let mut chars = json.chars();
        let mut in_string = false;
        let mut escaped = false;
        while let Some(ch) = chars.next() {
            if in_string {
                out.push(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            if ch == '"' {
                in_string = true;
                out.push(ch);
                continue;
            }
            if ch == ',' {
                let mut lookahead = chars.clone().peekable();
                while matches!(lookahead.peek(), Some(c) if c.is_whitespace()) {
                    lookahead.next();
                }
                if matches!(lookahead.peek(), Some('}' | ']')) {
                    continue;
                }
            }
            out.push(ch);
        }
        out
    }

    /// `Err` rather than a panic, so each `#[test]` fn's own body is where the assertion - and the
    /// `panicked at` site a mutation has to break - actually lives, not a shared helper three
    /// frames up.
    fn question_error(json: &str) -> Option<String> {
        let json = drop_trailing_commas(json);
        serde_json::from_str::<QuestionBody>(&json)
            .err()
            .map(|e| format!("{e}\nbody: {json}"))
    }

    #[test]
    fn the_healthcheck_s_two_questions_parse_as_the_current_wire_shape() {
        let source = read("demo/healthcheck.py");
        if let Some(e) = question_error(json_object_after(&source, "_A_REAL_QUESTION = ")) {
            panic!("demo/healthcheck.py _A_REAL_QUESTION does not parse as QuestionBody: {e}");
        }
        if let Some(e) = question_error(json_object_after(&source, "_AN_UNANSWERABLE_QUESTION = ")) {
            panic!("demo/healthcheck.py _AN_UNANSWERABLE_QUESTION does not parse as QuestionBody: {e}");
        }
    }

    #[test]
    fn the_release_smoke_test_s_two_questions_parse_as_the_current_wire_shape() {
        let source = read(".github/serve-smoke.sh");
        let bodies: Vec<&str> = source
            .match_indices("-d '{")
            .map(|(index, _)| {
                let rest = source
                    .get(index + 4..)
                    .expect("`-d '{` is ASCII, so 4 bytes past its start is a char boundary");
                let end = rest.find("}')\"").expect("a -d '...' body ends with }')\"") + 1;
                rest.get(..end)
                    .expect("`end` sits on the `}` byte `find` located, a char boundary")
            })
            .collect();
        assert_eq!(bodies.len(), 2, "serve-smoke.sh should send exactly two -d '{{...}}' bodies");
        let mut labels = ["the answer probe", "the refusal probe"].into_iter();
        for body in bodies {
            let label = labels.next().expect("two labels for two bodies");
            if let Some(e) = question_error(body) {
                panic!("{label} does not parse as QuestionBody: {e}");
            }
        }
    }
}
