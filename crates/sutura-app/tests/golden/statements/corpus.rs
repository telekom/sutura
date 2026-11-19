//! One query corpus for the renderer axis and every real engine, measured against CSV rows.

pub(super) struct FixtureRow {
    pub(super) date: String,
    pub(super) key: i64,
    pub(super) voice: f64,
}

pub(super) struct SubscriptionRow {
    pub(super) month: String,
    pub(super) key: i64,
}

#[derive(Clone, Copy)]
pub(super) enum JoinRows {
    All,
    LeftFilter,
    RightFilter,
    OrderedWindow,
    OrderedKeys,
    NestedWindow,
    LimitedInput,
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
    Joined(JoinRows),
    BareJoined { shape: JoinRows, right_alias: bool },
}

impl Expected {
    pub(super) fn columns(self) -> Vec<String> {
        let names: &[&str] = match self {
            Self::Keys(_) | Self::Limited { wide: false } => &["key"],
            Self::Limited { wide: true } => &["key", "voice"],
            Self::Count(_) => &["n"],
            Self::Grouped => &["key", "n"],
            Self::Joined(_) => &["left_key", "right_key", "usage_day", "billing_month"],
            Self::BareJoined { right_alias: false, .. } => &["subscription_key", "subscription_key"],
            Self::BareJoined { right_alias: true, .. } => &["subscription_key", "right_key"],
        };
        names.iter().map(|name| (*name).to_owned()).collect()
    }
}

pub(super) struct Case {
    pub(super) name: &'static str,
    projection: &'static str,
    from: &'static str,
    pub(super) expected: Expected,
}

impl Case {
    pub(super) fn sql(&self, table: &str, subscriptions: &str) -> String {
        format!("SELECT {} FROM {}", self.projection, self.from)
            .replace("%TABLE%", table)
            .replace("%SUBSCRIPTIONS%", subscriptions)
    }
}

const KEY: &str = "CAST(\"subscription_key\" AS BIGINT) AS \"key\"";
const COUNT: &str = "COUNT(*) AS \"n\"";
const JOINED: &str = "CAST(\"a\".\"subscription_key\" AS BIGINT) AS \"left_key\", CAST(\"b\".\"subscription_key\" AS BIGINT) AS \"right_key\", CAST(EXTRACT(DAY FROM \"a\".\"usage_date\") AS BIGINT) AS \"usage_day\", CAST(EXTRACT(MONTH FROM \"b\".\"month\") AS BIGINT) AS \"billing_month\"";

pub(super) const CASES: &[Case] = &[
    Case {
        name: "bare_join_left_filter_limit",
        projection: "\"a\".\"subscription_key\", \"b\".\"subscription_key\"",
        from: "%TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 WHERE \"a\".\"voice_min\" = 0 LIMIT 200",
        expected: Expected::BareJoined {
            shape: JoinRows::LeftFilter,
            right_alias: false,
        },
    },
    Case {
        name: "bare_join_right_filter",
        projection: "\"a\".\"subscription_key\", \"b\".\"subscription_key\"",
        from: "%TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 WHERE \"b\".\"month\" >= DATE '2026-04-01'",
        expected: Expected::BareJoined {
            shape: JoinRows::RightFilter,
            right_alias: false,
        },
    },
    Case {
        name: "bare_join_explicit_alias",
        projection: "\"a\".\"subscription_key\", \"b\".\"subscription_key\" AS \"right_key\"",
        from: "%TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 WHERE \"a\".\"voice_min\" = 0 LIMIT 200",
        expected: Expected::BareJoined {
            shape: JoinRows::LeftFilter,
            right_alias: true,
        },
    },
    Case {
        name: "bare_join_ordered_limit",
        projection: "\"a\".\"subscription_key\", \"b\".\"subscription_key\"",
        from: "%TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 WHERE \"a\".\"voice_min\" = 0 ORDER BY \"a\".\"subscription_key\", \"b\".\"subscription_key\" LIMIT 200",
        expected: Expected::BareJoined {
            shape: JoinRows::OrderedKeys,
            right_alias: false,
        },
    },
    // The joined fixture fits below 200 rows, so unordered LIMIT keeps the full multiset.
    Case {
        name: "join_left_filter_limit",
        projection: JOINED,
        from: "%TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 WHERE \"a\".\"voice_min\" = 0 LIMIT 200",
        expected: Expected::Joined(JoinRows::LeftFilter),
    },
    Case {
        name: "join_right_filter_limit",
        projection: JOINED,
        from: "%TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 WHERE \"b\".\"month\" >= DATE '2026-04-01' LIMIT 200",
        expected: Expected::Joined(JoinRows::RightFilter),
    },
    Case {
        name: "join_no_filter_limit",
        projection: JOINED,
        from: "%TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 LIMIT 200",
        expected: Expected::Joined(JoinRows::All),
    },
    Case {
        name: "join_unaliased_filter_limit",
        projection: "CAST(%TABLE%.\"subscription_key\" AS BIGINT) AS \"left_key\", CAST(%SUBSCRIPTIONS%.\"subscription_key\" AS BIGINT) AS \"right_key\", CAST(EXTRACT(DAY FROM %TABLE%.\"usage_date\") AS BIGINT) AS \"usage_day\", CAST(EXTRACT(MONTH FROM %SUBSCRIPTIONS%.\"month\") AS BIGINT) AS \"billing_month\"",
        from: "%TABLE% JOIN %SUBSCRIPTIONS% ON %TABLE%.\"subscription_key\" = %SUBSCRIPTIONS%.\"subscription_key\" AND %TABLE%.\"subscription_key\" BETWEEN 1001 AND 1002 WHERE %TABLE%.\"voice_min\" = 0 LIMIT 200",
        expected: Expected::Joined(JoinRows::LeftFilter),
    },
    Case {
        name: "join_ordered_filter_limit",
        projection: JOINED,
        from: "%TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 WHERE \"a\".\"voice_min\" = 0 ORDER BY \"usage_day\" DESC, \"billing_month\" ASC LIMIT 5 OFFSET 2",
        expected: Expected::Joined(JoinRows::OrderedWindow),
    },
    Case {
        name: "join_filter_without_limit",
        projection: JOINED,
        from: "%TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 WHERE \"a\".\"voice_min\" = 0",
        expected: Expected::Joined(JoinRows::LeftFilter),
    },
    Case {
        name: "join_nested_windows",
        projection: "\"q\".\"left_key\", \"q\".\"right_key\", \"q\".\"usage_day\", \"q\".\"billing_month\"",
        from: "(SELECT CAST(\"a\".\"subscription_key\" AS BIGINT) AS \"left_key\", CAST(\"b\".\"subscription_key\" AS BIGINT) AS \"right_key\", CAST(EXTRACT(DAY FROM \"a\".\"usage_date\") AS BIGINT) AS \"usage_day\", CAST(EXTRACT(MONTH FROM \"b\".\"month\") AS BIGINT) AS \"billing_month\" FROM %TABLE% \"a\" JOIN %SUBSCRIPTIONS% \"b\" ON \"a\".\"subscription_key\" + 1 = \"b\".\"subscription_key\" AND \"a\".\"subscription_key\" BETWEEN 1001 AND 1002 WHERE \"a\".\"voice_min\" = 0 ORDER BY \"usage_day\" DESC, \"billing_month\" ASC LIMIT 8 OFFSET 1) \"q\" ORDER BY \"billing_month\" DESC, \"usage_day\" ASC LIMIT 3 OFFSET 2",
        expected: Expected::Joined(JoinRows::NestedWindow),
    },
    Case {
        name: "join_limited_input",
        projection: "CAST(\"a\".\"subscription_key\" AS BIGINT) AS \"left_key\", CAST(\"b\".\"subscription_key\" AS BIGINT) AS \"right_key\", CAST(EXTRACT(DAY FROM \"a\".\"usage_date\") AS BIGINT) AS \"usage_day\", CAST(0 AS BIGINT) AS \"billing_month\"",
        from: "%TABLE% \"a\" JOIN (SELECT \"subscription_key\" FROM %SUBSCRIPTIONS% WHERE \"subscription_key\" = 1003 LIMIT 2) \"b\" ON \"a\".\"subscription_key\" + 1 = \"b\".\"subscription_key\" WHERE \"a\".\"voice_min\" = 0 LIMIT 200",
        expected: Expected::Joined(JoinRows::LimitedInput),
    },
    Case {
        name: "plain_filter",
        projection: KEY,
        from: "%TABLE% WHERE \"subscription_key\" = 1002",
        expected: Expected::Keys(Predicate::Key),
    },
    Case {
        name: "aliased_filter",
        projection: "CAST(\"k\".\"subscription_key\" AS BIGINT) AS \"key\"",
        from: "%TABLE% \"k\" WHERE \"k\".\"subscription_key\" = 1002",
        expected: Expected::Keys(Predicate::Key),
    },
    Case {
        name: "not_null_and_filter",
        projection: KEY,
        from: "%TABLE% WHERE \"usage_date\" IS NOT NULL AND \"subscription_key\" = 1002",
        expected: Expected::Keys(Predicate::Key),
    },
    Case {
        name: "count_column_filter",
        projection: "COUNT(\"subscription_key\") AS \"n\"",
        from: "%TABLE% WHERE \"voice_min\" = 0",
        expected: Expected::Count(Predicate::ZeroVoice),
    },
    Case {
        name: "ordered_filter_limit",
        projection: KEY,
        from: "%TABLE% WHERE \"subscription_key\" = 1002 ORDER BY \"subscription_key\" LIMIT 2",
        expected: Expected::Limited { wide: false },
    },
    Case {
        name: "grouped_count_star",
        projection: "CAST(\"subscription_key\" AS BIGINT) AS \"key\", COUNT(*) AS \"n\"",
        from: "%TABLE% GROUP BY \"subscription_key\"",
        expected: Expected::Grouped,
    },
    Case {
        name: "date_comparison",
        projection: KEY,
        from: "%TABLE% WHERE \"usage_date\" >= DATE '2026-06-02'",
        expected: Expected::Keys(Predicate::Since),
    },
    Case {
        name: "timestamp_comparison",
        projection: KEY,
        from: "%TABLE% WHERE \"usage_date\" >= TIMESTAMP '2026-06-02 00:00:00'",
        expected: Expected::Keys(Predicate::Since),
    },
    Case {
        name: "date_between",
        projection: KEY,
        from: "%TABLE% WHERE \"usage_date\" BETWEEN DATE '2026-06-01' AND DATE '2026-06-02'",
        expected: Expected::Keys(Predicate::Between),
    },
    Case {
        name: "date_part_year",
        projection: KEY,
        from: "%TABLE% WHERE DATE_PART('year', \"usage_date\") = 2026",
        expected: Expected::Keys(Predicate::Year),
    },
    Case {
        name: "extract_year",
        projection: KEY,
        from: "%TABLE% WHERE EXTRACT(YEAR FROM \"usage_date\") = 2026",
        expected: Expected::Keys(Predicate::Year),
    },
    Case {
        name: "count_star_filter",
        projection: COUNT,
        from: "%TABLE% WHERE \"voice_min\" = 0",
        expected: Expected::Count(Predicate::ZeroVoice),
    },
    Case {
        name: "count_one_filter",
        projection: "COUNT(1) AS \"n\"",
        from: "%TABLE% WHERE \"voice_min\" = 0",
        expected: Expected::Count(Predicate::ZeroVoice),
    },
    Case {
        name: "filter_limit",
        projection: KEY,
        from: "%TABLE% WHERE \"subscription_key\" = 1002 LIMIT 2",
        expected: Expected::Limited { wide: false },
    },
    Case {
        name: "filter_wide_limit",
        projection: "CAST(\"subscription_key\" AS BIGINT) AS \"key\", CAST(\"voice_min\" AS BIGINT) AS \"voice\"",
        from: "%TABLE% WHERE \"subscription_key\" = 1002 LIMIT 2",
        expected: Expected::Limited { wide: true },
    },
    Case {
        name: "count_star",
        projection: COUNT,
        from: "%TABLE% ",
        expected: Expected::Count(Predicate::All),
    },
    Case {
        name: "aliased_filter_limit",
        projection: "CAST(\"k\".\"subscription_key\" AS BIGINT) AS \"key\"",
        from: "%TABLE% \"k\" WHERE \"k\".\"subscription_key\" = 1002 LIMIT 2",
        expected: Expected::Limited { wide: false },
    },
    Case {
        name: "aliased_filter_wide_limit",
        projection: "CAST(\"k\".\"subscription_key\" AS BIGINT) AS \"key\", CAST(\"k\".\"voice_min\" AS BIGINT) AS \"voice\"",
        from: "%TABLE% \"k\" WHERE \"k\".\"subscription_key\" = 1002 LIMIT 2",
        expected: Expected::Limited { wide: true },
    },
    Case {
        name: "aliased_ordered_filter_limit",
        projection: "CAST(\"k\".\"subscription_key\" AS BIGINT) AS \"key\"",
        from: "%TABLE% \"k\" WHERE \"k\".\"subscription_key\" = 1002 ORDER BY \"k\".\"subscription_key\" LIMIT 2",
        expected: Expected::Limited { wide: false },
    },
    Case {
        name: "correlated_exists",
        projection: "CAST(\"k\".\"subscription_key\" AS BIGINT) AS \"key\"",
        from: "%TABLE% \"k\" WHERE EXISTS (SELECT 1 FROM %TABLE% \"x\" WHERE \"x\".\"subscription_key\" = \"k\".\"subscription_key\")",
        expected: Expected::Keys(Predicate::All),
    },
    Case {
        name: "correlated_exists_filter",
        projection: "CAST(\"k\".\"subscription_key\" AS BIGINT) AS \"key\"",
        from: "%TABLE% \"k\" WHERE EXISTS (SELECT 1 FROM %TABLE% \"x\" WHERE \"x\".\"subscription_key\" = \"k\".\"subscription_key\" AND \"x\".\"subscription_key\" = 1002)",
        expected: Expected::Keys(Predicate::Key),
    },
    Case {
        name: "self_in_filter",
        projection: "CAST(\"k\".\"subscription_key\" AS BIGINT) AS \"key\"",
        from: "%TABLE% \"k\" WHERE \"k\".\"subscription_key\" IN (SELECT \"x\".\"subscription_key\" FROM %TABLE% \"x\" WHERE \"x\".\"subscription_key\" = 1002)",
        expected: Expected::Keys(Predicate::Key),
    },
    Case {
        name: "self_in_filter_wide_limit",
        projection: "CAST(\"k\".\"subscription_key\" AS BIGINT) AS \"key\", CAST(\"k\".\"voice_min\" AS BIGINT) AS \"voice\"",
        from: "%TABLE% \"k\" WHERE \"k\".\"subscription_key\" IN (SELECT \"x\".\"subscription_key\" FROM %TABLE% \"x\" WHERE \"x\".\"subscription_key\" = 1002) LIMIT 2",
        expected: Expected::Limited { wide: true },
    },
    Case {
        name: "correlated_not_exists_preserves_outer_filter",
        projection: "CAST(\"k\".\"subscription_key\" AS BIGINT) AS \"key\"",
        from: "%TABLE% \"k\" WHERE \"k\".\"subscription_key\" = 1002 AND NOT EXISTS (SELECT 1 FROM %TABLE% \"x\" WHERE \"x\".\"subscription_key\" = \"k\".\"subscription_key\" AND \"x\".\"voice_min\" < 0)",
        expected: Expected::Keys(Predicate::Key),
    },
];
