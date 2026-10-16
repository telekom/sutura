CREATE TABLE dim_customer (
    customer_key Int64,
    customer_id String,
    segment String,
    region String
) ENGINE = MergeTree ORDER BY customer_key;
INSERT INTO dim_customer SELECT * FROM file('dim_customer.csv', 'CSVWithNames');
