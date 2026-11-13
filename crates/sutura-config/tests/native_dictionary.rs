#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use sutura_config::{Environment, Settings, Sources};

    fn load(selection: &str) -> Result<Settings, sutura_config::SettingsLoadError> {
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!(
            "security:
  identity: single-user
  single_user_because: a test
sources:
  warehouse:
    kind: files
    data_dir: /srv/data
    posture: shared-service-user
catalogs:
  - name: dictionary
    kind: rdbms
    version: dict-1
    source_alias: warehouse
{selection}
    connection:
      host: 127.0.0.1
      port: 5432
      database: dictionary
      user: reader
      password_file: /run/secrets/dictionary
      transport_mode: plaintext
"
        )))
    }

    const NATIVE: &str = "    dictionary_source: native_dictionary\n    include: [{schema: public, table: 'event*'}]";

    fn refusal(selection: &str) -> String {
        let error = load(selection).expect_err("the native declaration is refused");
        let mut chain = error.to_string();
        let mut cause = error.source();
        while let Some(next) = cause {
            chain.push_str(": ");
            chain.push_str(&next.to_string());
            cause = next.source();
        }
        chain
    }

    #[test]
    fn a_native_dictionary_selection_loads_without_documentation_view_keys() {
        load(&format!(
            "{NATIVE}\n    exclude_tables: [events_old]\n    exclude_columns: ['secret*']\n    exclude_types: [jsonb]"
        ))
        .expect("native metadata needs no view environment or schema");
    }

    #[test]
    fn native_dictionary_patterns_are_bounded_and_include_pairs_are_required() {
        load(NATIVE).expect("the positive control is a complete native declaration");
        for selection in [
            String::from("    dictionary_source: native_dictionary"),
            String::from("    dictionary_source: native_dictionary\n    include: []"),
            format!(
                "    dictionary_source: native_dictionary\n    include: [{{schema: '{}', table: '*'}}]",
                "x".repeat(129)
            ),
            format!("{NATIVE}\n    exclude_columns: [{}]", vec!["secret"; 129].join(", ")),
        ] {
            let error = refusal(&selection);
            assert!(error.contains("native dictionary"), "{error}");
        }
    }

    #[test]
    fn native_and_view_dictionary_keys_cannot_be_mixed() {
        load(NATIVE).expect("the positive control opens the native mode");
        for key in [
            "environment: test",
            "dictionary_schema: public",
            "live_row_predicate: {column: removed, operator: is_null}",
        ] {
            let error = refusal(&format!("{NATIVE}\n    {key}"));
            assert!(error.contains("native dictionaries read no documentation"), "{error}");
        }
        let error = refusal("    environment: test\n    include: [{schema: public, table: '*'}]");
        assert!(error.contains("require dictionary_source"), "{error}");
    }
}
