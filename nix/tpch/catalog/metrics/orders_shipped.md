---
kind: metric
name: orders_shipped
model: lineitem
measure:
  simple: { aggregate: count_distinct, column: l_orderkey }
time_column: l_shipdate
grains: [day, week, month, quarter, year]
dimensions:
  - name: ship_mode
    column: l_shipmode
    values:
      - "AIR"
      - "FOB"
      - "MAIL"
      - "RAIL"
      - "REG AIR"
      - "SHIP"
      - "TRUCK"
  - name: order_priority
    column: o_orderpriority
    via: lineitem_to_order
    values:
      - "1-URGENT"
      - "2-HIGH"
      - "3-MEDIUM"
      - "4-NOT SPECIFIED"
      - "5-LOW"
  - name: market_segment
    column: c_mktsegment
    via:
      - lineitem_to_order
      - order_to_customer
    values:
      - "AUTOMOBILE"
      - "BUILDING"
      - "FURNITURE"
      - "HOUSEHOLD"
      - "MACHINERY"
  - name: customer_region
    column: r_name
    via:
      - lineitem_to_order
      - order_to_customer
      - customer_to_nation
      - nation_to_region
    values:
      - "AFRICA"
      - "AMERICA"
      - "ASIA"
      - "EUROPE"
      - "MIDDLE EAST"
  - name: part_brand
    column: p_brand
    via: lineitem_to_part
    values:
      - "Brand#11"
      - "Brand#12"
      - "Brand#13"
      - "Brand#14"
      - "Brand#15"
      - "Brand#21"
      - "Brand#22"
      - "Brand#23"
      - "Brand#24"
      - "Brand#25"
      - "Brand#31"
      - "Brand#32"
      - "Brand#33"
      - "Brand#34"
      - "Brand#35"
      - "Brand#41"
      - "Brand#42"
      - "Brand#43"
      - "Brand#44"
      - "Brand#45"
      - "Brand#51"
      - "Brand#52"
      - "Brand#53"
      - "Brand#54"
      - "Brand#55"
  - name: supplier_region
    column: r_name
    via:
      - lineitem_to_supplier
      - supplier_to_nation
      - nation_to_region
    values:
      - "AFRICA"
      - "AMERICA"
      - "ASIA"
      - "EUROPE"
      - "MIDDLE EAST"
audience: open
---

Distinct orders with a line shipped in the period. Across systems the key is pulled up rather than summed.
