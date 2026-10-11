-- The metrics-layer example's tables in the `cube` profile's ClickHouse, loaded at first start
-- after `examples/cube/clickhouse/init.sql`. One shared schema: Cube decides which rows of `orders`
-- each caller reads, from the `regions` claim of the caller's own token.
CREATE TABLE default.orders (
    order_id String, customer_id String, amount_cents Int64, order_date Date, status String,
    region String
) ENGINE = MergeTree ORDER BY tuple();
INSERT INTO default.orders VALUES
    ('O1', 'C1', 200000, '2026-06-01', 'active', 'north'),
    ('O2', 'C2', 150000, '2026-06-01', 'active', 'south'),
    ('O3', 'C1', 62345, '2026-06-01', 'active', 'north'),
    ('O4', 'C2', 999999, '2026-06-01', 'cancelled', 'south'),
    ('O5', 'C1', 111111, '2026-05-01', 'active', 'north');
CREATE TABLE default.customers (customer_id String, segment String) ENGINE = MergeTree ORDER BY tuple();
INSERT INTO default.customers VALUES ('C1', 'retail'), ('C2', 'wholesale');
