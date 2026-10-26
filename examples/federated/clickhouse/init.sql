CREATE TABLE dim_customer (
    customer_key Int64,
    customer_id String,
    segment String,
    region String
) ENGINE = MergeTree ORDER BY customer_key;
INSERT INTO dim_customer SELECT * FROM file('dim_customer.csv', 'CSVWithNames');

CREATE TABLE fct_customer_monthly (
    month Date,
    customer_key Int64,
    segment String
) ENGINE = MergeTree ORDER BY (month, customer_key);
INSERT INTO fct_customer_monthly VALUES
    ('2026-06-01', 1, 'business'),
    ('2026-06-01', 2, 'consumer'),
    ('2026-06-01', 3, 'consumer'),
    ('2026-06-01', 4, 'wholesale');
