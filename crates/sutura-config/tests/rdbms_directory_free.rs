#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use sutura_config::{CatalogKind, Environment, Settings, Sources};

    #[test]
    fn an_rdbms_catalog_needs_no_directory_paths() {
        let overlay = "
security:
  identity: single-user
  single_user_because: a test
sources:
  warehouse:
    kind: files
    data_dir: /srv/data
    posture: shared-service-user
catalogs:
  - name: dict
    kind: rdbms
    version: dict-1
    environment: prod
    source_alias: warehouse
    max_dictionary_rows: 1000
    connection:
      host: 127.0.0.1
      port: 5432
      database: dictionary
      user: reader
      password_file: /run/secrets/dictionary
      transport_mode: plaintext
";
        let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
            .expect("a live catalog does not need directory paths");
        let catalog = settings.catalogs().each().next().expect("one catalog");
        assert_eq!(catalog.kind(), CatalogKind::Rdbms);
    }
}
