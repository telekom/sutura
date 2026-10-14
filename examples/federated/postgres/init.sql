CREATE TABLE fct_subscription_monthly (
    month DATE NOT NULL,
    subscription_key BIGINT NOT NULL,
    customer_key BIGINT NOT NULL,
    product_key BIGINT NOT NULL,
    status TEXT NOT NULL,
    mrr_cents BIGINT NOT NULL,
    churned_in_month BOOLEAN NOT NULL,
    contract_term TEXT NOT NULL
);
COPY fct_subscription_monthly FROM '/data/fct_subscription_monthly.csv' WITH (FORMAT csv, HEADER true);
