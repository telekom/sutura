//! One query corpus for the renderer axis and every real engine, measured against CSV rows.

pub(super) struct FixtureRow {
    pub(super) date: String,
    pub(super) key: i64,
    pub(super) voice: f64,
}

#[derive(Clone, Copy)]
pub(super) enum Predicate {
    All,
    Key,
    ZeroVoice,
    Since,
    Between,
    Year,
}

impl Predicate {
    pub(super) fn accepts(self, row: &FixtureRow) -> bool {
        match self {
            Self::All => true,
            Self::Key => row.key == 1002,
            Self::ZeroVoice => row.voice == 0.0,
            Self::Since => row.date.as_str() >= "2026-06-02",
            Self::Between => ("2026-06-01"..="2026-06-02").contains(&row.date.as_str()),
            Self::Year => row.date.starts_with("2026-"),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Expected {
    Keys(Predicate),
    Count(Predicate),
    Limited { wide: bool },
    Grouped,
}

impl Expected {
    pub(super) fn columns(self) -> Vec<String> {
        let names: &[&str] = match self {
            Self::Keys(_) | Self::Limited { wide: false } => &["key"],
            Self::Limited { wide: true } => &["key", "voice"],
            Self::Count(_) => &["n"],
            Self::Grouped => &["key", "n"],
        };
        names.iter().map(|name| (*name).to_owned()).collect()
    }
}

pub(super) struct Case {
    pub(super) name: &'static str,
    projection: &'static str,
    after_table: &'static str,
    pub(super) expected: Expected,
}

impl Case {
    pub(super) fn sql(&self, table: &str) -> String {
        format!("SELECT {} FROM {table} {}", self.projection, self.after_table)
    }
}

const KEY: &str = "CAST(\"subscription_key\" AS BIGINT) AS \"key\"";
const COUNT: &str = "COUNT(*) AS \"n\"";

pub(super) const CASES: &[Case] = &[
    Case {
        name: "plain_filter",
        projection: KEY,
        after_table: "WHERE \"subscription_key\" = 1002",
        expected: Expected::Keys(Predicate::Key),
    },
    Case {
        name: "not_null_and_filter",
        projection: KEY,
        after_table: "WHERE \"usage_date\" IS NOT NULL AND \"subscription_key\" = 1002",
        expected: Expected::Keys(Predicate::Key),
    },
    Case {
        name: "count_column_filter",
        projection: "COUNT(\"subscription_key\") AS \"n\"",
        after_table: "WHERE \"voice_min\" = 0",
        expected: Expected::Count(Predicate::ZeroVoice),
    },
    Case {
        name: "ordered_filter_limit",
        projection: KEY,
        after_table: "WHERE \"subscription_key\" = 1002 ORDER BY \"subscription_key\" LIMIT 2",
        expected: Expected::Limited { wide: false },
    },
    Case {
        name: "grouped_count_star",
        projection: "CAST(\"subscription_key\" AS BIGINT) AS \"key\", COUNT(*) AS \"n\"",
        after_table: "GROUP BY \"subscription_key\"",
        expected: Expected::Grouped,
    },
    Case {
        name: "date_comparison",
        projection: KEY,
        after_table: "WHERE \"usage_date\" >= DATE '2026-06-02'",
        expected: Expected::Keys(Predicate::Since),
    },
    Case {
        name: "timestamp_comparison",
        projection: KEY,
        after_table: "WHERE \"usage_date\" >= TIMESTAMP '2026-06-02 00:00:00'",
        expected: Expected::Keys(Predicate::Since),
    },
    Case {
        name: "date_between",
        projection: KEY,
        after_table: "WHERE \"usage_date\" BETWEEN DATE '2026-06-01' AND DATE '2026-06-02'",
        expected: Expected::Keys(Predicate::Between),
    },
    Case {
        name: "date_part_year",
        projection: KEY,
        after_table: "WHERE DATE_PART('year', \"usage_date\") = 2026",
        expected: Expected::Keys(Predicate::Year),
    },
    Case {
        name: "extract_year",
        projection: KEY,
        after_table: "WHERE EXTRACT(YEAR FROM \"usage_date\") = 2026",
        expected: Expected::Keys(Predicate::Year),
    },
    Case {
        name: "count_star_filter",
        projection: COUNT,
        after_table: "WHERE \"voice_min\" = 0",
        expected: Expected::Count(Predicate::ZeroVoice),
    },
    Case {
        name: "count_one_filter",
        projection: "COUNT(1) AS \"n\"",
        after_table: "WHERE \"voice_min\" = 0",
        expected: Expected::Count(Predicate::ZeroVoice),
    },
    Case {
        name: "filter_limit",
        projection: KEY,
        after_table: "WHERE \"subscription_key\" = 1002 LIMIT 2",
        expected: Expected::Limited { wide: false },
    },
    Case {
        name: "filter_wide_limit",
        projection: "CAST(\"subscription_key\" AS BIGINT) AS \"key\", CAST(\"voice_min\" AS BIGINT) AS \"voice\"",
        after_table: "WHERE \"subscription_key\" = 1002 LIMIT 2",
        expected: Expected::Limited { wide: true },
    },
    Case {
        name: "count_star",
        projection: COUNT,
        after_table: "",
        expected: Expected::Count(Predicate::All),
    },
];
