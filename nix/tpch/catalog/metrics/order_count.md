---
kind: metric
name: order_count
model: orders
measure:
  simple: { aggregate: count, column: o_orderkey }
time_column: o_orderdate
grains: [day, week, month, quarter, year]
dimensions:
  - name: order_priority
    column: o_orderpriority
    values:
      - "1-URGENT"
      - "2-HIGH"
      - "3-MEDIUM"
      - "4-NOT SPECIFIED"
      - "5-LOW"
  - name: order_status
    column: o_orderstatus
    values:
      - "F"
      - "O"
      - "P"
  - name: market_segment
    column: c_mktsegment
    via: order_to_customer
    values:
      - "AUTOMOBILE"
      - "BUILDING"
      - "FURNITURE"
      - "HOUSEHOLD"
      - "MACHINERY"
  - name: customer_nation
    column: n_name
    via:
      - order_to_customer
      - customer_to_nation
    values:
      - "ALGERIA"
      - "ARGENTINA"
      - "BRAZIL"
      - "CANADA"
      - "CHINA"
      - "EGYPT"
      - "ETHIOPIA"
      - "FRANCE"
      - "GERMANY"
      - "INDIA"
      - "INDONESIA"
      - "IRAN"
      - "IRAQ"
      - "JAPAN"
      - "JORDAN"
      - "KENYA"
      - "MOROCCO"
      - "MOZAMBIQUE"
      - "PERU"
      - "ROMANIA"
      - "RUSSIA"
      - "SAUDI ARABIA"
      - "UNITED KINGDOM"
      - "UNITED STATES"
      - "VIETNAM"
  - name: customer_region
    column: r_name
    via:
      - order_to_customer
      - customer_to_nation
      - nation_to_region
    values:
      - "AFRICA"
      - "AMERICA"
      - "ASIA"
      - "EUROPE"
      - "MIDDLE EAST"
audience: open
---

Orders placed in the period.
