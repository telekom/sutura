//! Fixed metadata queries; configuration contributes only bound schema values.

fn schemas(count: usize, column: &str, marker: char) -> String {
    (1..=count)
        .map(|index| format!("{column} LIKE {marker}{index} ESCAPE '\\'"))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(crate) fn postgres(count: usize, limit: u64) -> String {
    let schemas = schemas(count, "c.table_schema", '$');
    format!(
        "SELECT 'native'::text AS environment, NULL::text AS catalog_name, \
         c.table_schema::text AS schema_name, c.table_name::text AS table_name, \
         c.table_name::text AS model_name, pg_catalog.obj_description(t.oid, 'pg_class') AS table_description, \
         c.column_name::text AS column_name, pg_catalog.format_type(a.atttypid, a.atttypmod) AS column_type, \
         pg_catalog.col_description(t.oid, a.attnum) AS column_description, false AS is_primary_key, \
         CASE WHEN a.attcollation = 0 OR a.atttypid NOT IN ( \
                'pg_catalog.text'::regtype, 'pg_catalog.varchar'::regtype, 'pg_catalog.bpchar'::regtype, 'pg_catalog.name'::regtype) \
              THEN NULL \
              WHEN c.collation_name IS NULL THEN 'default' \
              ELSE c.collation_schema::text || '.' || c.collation_name::text END AS column_collation \
         FROM information_schema.columns c \
         JOIN pg_catalog.pg_namespace n ON n.nspname = c.table_schema \
         JOIN pg_catalog.pg_class t ON t.relnamespace = n.oid AND t.relname = c.table_name \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = t.oid AND a.attname = c.column_name \
         WHERE ({schemas}) ORDER BY c.table_schema, c.table_name, c.ordinal_position LIMIT {limit}"
    )
}

#[cfg(feature = "live-oracle")]
pub(crate) fn oracle(count: usize, limit: u64) -> String {
    let schemas = schemas(count, "c.OWNER", ':');
    format!(
        "SELECT 'native', CAST(NULL AS VARCHAR2(128)), c.OWNER, c.TABLE_NAME, c.TABLE_NAME, t.COMMENTS, c.COLUMN_NAME, \
         CASE WHEN c.DATA_TYPE = 'NUMBER' AND c.DATA_PRECISION IS NOT NULL AND c.DATA_SCALE IS NOT NULL \
           THEN 'NUMBER(' || TO_CHAR(c.DATA_PRECISION, 'FM9990') || ',' || TO_CHAR(c.DATA_SCALE, 'FM9990') || ')' \
         WHEN c.DATA_TYPE = 'FLOAT' AND c.DATA_PRECISION IS NOT NULL \
           THEN 'FLOAT(' || TO_CHAR(c.DATA_PRECISION, 'FM9990') || ')' \
         WHEN c.DATA_TYPE IN ('VARCHAR2', 'CHAR', 'NVARCHAR2', 'NCHAR') \
           THEN c.DATA_TYPE || '(' || CASE WHEN c.CHAR_USED = 'B' THEN TO_CHAR(c.DATA_LENGTH, 'FM9999990') || ' BYTE)' ELSE TO_CHAR(c.CHAR_LENGTH, 'FM9999990') || ' CHAR)' END \
         WHEN c.DATA_TYPE = 'RAW' THEN 'RAW(' || TO_CHAR(c.DATA_LENGTH, 'FM9999990') || ')' \
         ELSE c.DATA_TYPE END, d.COMMENTS, 0 \
         FROM ALL_TAB_COLUMNS c \
         LEFT JOIN ALL_TAB_COMMENTS t ON t.OWNER = c.OWNER AND t.TABLE_NAME = c.TABLE_NAME \
         LEFT JOIN ALL_COL_COMMENTS d ON d.OWNER = c.OWNER AND d.TABLE_NAME = c.TABLE_NAME AND d.COLUMN_NAME = c.COLUMN_NAME \
         WHERE ({schemas}) ORDER BY c.OWNER, c.TABLE_NAME, c.COLUMN_ID FETCH FIRST {limit} ROWS ONLY"
    )
}

/// `conenforced` exists only from PostgreSQL 18; before it every constraint is enforced, so the
/// missing key reads as true.
///
/// Every row is one ordinal member of a complete enforced key. The referenced index must be
/// immediate, valid, nonpartial and without expressions; a unique index alone is not a constraint.
pub(crate) fn postgres_constraints(count: usize, limit: u64) -> String {
    let schemas = schemas(count, "n.nspname", '$');
    format!(
        "WITH admitted AS ( \
         SELECT f.oid, f.conname, f.contype, f.conrelid, f.confrelid, f.conkey, f.confkey, \
                u.conkey AS target_key, u.contype AS target_kind \
         FROM pg_catalog.pg_constraint f \
         JOIN pg_catalog.pg_class t ON t.oid = f.conrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace \
         LEFT JOIN pg_catalog.pg_constraint u ON f.contype = 'f' AND u.conrelid = f.confrelid \
              AND u.conindid = f.conindid AND u.contype IN ('p', 'u') \
         JOIN pg_catalog.pg_index i ON i.indexrelid = CASE WHEN f.contype = 'p' THEN f.conindid ELSE u.conindid END \
         WHERE ({schemas}) AND f.contype IN ('p', 'f') \
           AND COALESCE((to_jsonb(f)->>'conenforced')::boolean, true) AND f.convalidated AND NOT f.condeferrable \
           AND i.indisvalid AND i.indisunique AND i.indimmediate AND i.indpred IS NULL AND i.indexprs IS NULL \
           AND (f.contype = 'p' OR (COALESCE((to_jsonb(u)->>'conenforced')::boolean, true) AND u.convalidated AND NOT u.condeferrable \
                AND cardinality(f.confkey) = cardinality(u.conkey) \
                AND f.confkey @> u.conkey AND f.confkey <@ u.conkey))) \
         SELECT f.contype::text AS record_kind, f.oid::text AS constraint_id, f.conname::text AS constraint_name, \
                n.nspname::text AS schema_name, t.relname::text AS table_name, a.attname::text AS origin_column, \
                tn.nspname::text AS target_schema, tt.relname::text AS target_table, ta.attname::text AS target_column, \
                ka.attname::text AS target_key_column, member.ordinality::text AS ordinal, \
                cardinality(f.conkey)::text AS arity, f.target_kind::text AS target_kind \
         FROM admitted f JOIN pg_catalog.pg_class t ON t.oid = f.conrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace \
         CROSS JOIN LATERAL unnest(f.conkey) WITH ORDINALITY AS member(attnum, ordinality) \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = f.conrelid AND a.attnum = member.attnum \
         LEFT JOIN pg_catalog.pg_class tt ON tt.oid = f.confrelid \
         LEFT JOIN pg_catalog.pg_namespace tn ON tn.oid = tt.relnamespace \
         LEFT JOIN pg_catalog.pg_attribute ta ON ta.attrelid = f.confrelid AND ta.attnum = f.confkey[member.ordinality] \
         LEFT JOIN pg_catalog.pg_attribute ka ON ka.attrelid = f.confrelid AND ka.attnum = f.target_key[member.ordinality] \
         ORDER BY f.oid, member.ordinality LIMIT {limit}"
    )
}

#[cfg(feature = "live-oracle")]
pub(crate) fn oracle_constraints(count: usize, limit: u64) -> String {
    let schemas = schemas(count, "f.OWNER", ':');
    format!(
        "SELECT CASE f.CONSTRAINT_TYPE WHEN 'P' THEN 'p' ELSE 'f' END, \
         f.OWNER || '.' || f.CONSTRAINT_NAME, f.CONSTRAINT_NAME, f.OWNER, f.TABLE_NAME, c.COLUMN_NAME, \
         u.OWNER, u.TABLE_NAME, k.COLUMN_NAME, k.COLUMN_NAME, TO_CHAR(c.POSITION, 'FM99990'), \
         TO_CHAR((SELECT COUNT(*) FROM ALL_CONS_COLUMNS z WHERE z.OWNER = f.OWNER AND z.CONSTRAINT_NAME = f.CONSTRAINT_NAME), 'FM99990'), \
         CASE u.CONSTRAINT_TYPE WHEN 'P' THEN 'p' WHEN 'U' THEN 'u' END \
         FROM ALL_CONSTRAINTS f JOIN ALL_CONS_COLUMNS c ON c.OWNER = f.OWNER AND c.CONSTRAINT_NAME = f.CONSTRAINT_NAME \
         LEFT JOIN ALL_CONSTRAINTS u ON u.OWNER = f.R_OWNER AND u.CONSTRAINT_NAME = f.R_CONSTRAINT_NAME \
         LEFT JOIN ALL_CONS_COLUMNS k ON k.OWNER = u.OWNER AND k.CONSTRAINT_NAME = u.CONSTRAINT_NAME AND k.POSITION = c.POSITION \
         WHERE ({schemas}) AND f.CONSTRAINT_TYPE IN ('P', 'R') \
           AND f.STATUS = 'ENABLED' AND f.VALIDATED = 'VALIDATED' AND f.DEFERRABLE = 'NOT DEFERRABLE' \
           AND (f.CONSTRAINT_TYPE = 'P' OR (u.CONSTRAINT_TYPE IN ('P', 'U') \
                AND u.STATUS = 'ENABLED' AND u.VALIDATED = 'VALIDATED' AND u.DEFERRABLE = 'NOT DEFERRABLE' \
                AND (SELECT COUNT(*) FROM ALL_CONS_COLUMNS z WHERE z.OWNER = u.OWNER AND z.CONSTRAINT_NAME = u.CONSTRAINT_NAME) \
                  = (SELECT COUNT(*) FROM ALL_CONS_COLUMNS z WHERE z.OWNER = f.OWNER AND z.CONSTRAINT_NAME = f.CONSTRAINT_NAME))) \
         ORDER BY f.OWNER, f.CONSTRAINT_NAME, c.POSITION FETCH FIRST {limit} ROWS ONLY"
    )
}
