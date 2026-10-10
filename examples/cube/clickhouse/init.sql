-- The `cube` profile's ClickHouse: the single-player corpus as tables, loaded once at first start
-- from the CSV files `compose.services.yaml` places under `/corpus`.
CREATE TABLE default.fct_subscription_monthly (
    month Date, subscription_key Int64, customer_key Int64, product_key Int64, status String,
    mrr_cents Int64, churned_in_month Bool, contract_term String
) ENGINE = MergeTree ORDER BY tuple();
INSERT INTO default.fct_subscription_monthly FROM INFILE '/corpus/fct_subscription_monthly.csv' FORMAT CSVWithNames;
CREATE TABLE default.fct_usage_daily (
    usage_date Date, subscription_key Int64, data_gb Float64, voice_min Float64
) ENGINE = MergeTree ORDER BY tuple();
INSERT INTO default.fct_usage_daily FROM INFILE '/corpus/fct_usage_daily.csv' FORMAT CSVWithNames;
CREATE TABLE default.dim_customer (
    customer_key Int64, customer_id String, segment String, region String
) ENGINE = MergeTree ORDER BY tuple();
INSERT INTO default.dim_customer FROM INFILE '/corpus/dim_customer.csv' FORMAT CSVWithNames;
CREATE TABLE default.dim_product (
    product_key Int64, product_name String, product_family String
) ENGINE = MergeTree ORDER BY tuple();
INSERT INTO default.dim_product FROM INFILE '/corpus/dim_product.csv' FORMAT CSVWithNames;
CREATE TABLE default.dim_region (region String, sales_area String) ENGINE = MergeTree ORDER BY tuple();
INSERT INTO default.dim_region FROM INFILE '/corpus/dim_region.csv' FORMAT CSVWithNames;
