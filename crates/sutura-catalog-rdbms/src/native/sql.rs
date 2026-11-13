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
         pg_catalog.col_description(t.oid, a.attnum) AS column_description, false AS is_primary_key \
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
