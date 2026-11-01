---
kind: metric
name: quantity_shipped
model: lineitem
measure:
  simple: { aggregate: sum, column: l_quantity }
time_column: l_shipdate
grains: [day, week, month, quarter, year]
dimensions:
  - name: return_flag
    column: l_returnflag
    values:
      - "A"
      - "N"
      - "R"
  - name: line_status
    column: l_linestatus
    values:
      - "F"
      - "O"
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
  - name: ship_instruct
    column: l_shipinstruct
    values:
      - "COLLECT COD"
      - "DELIVER IN PERSON"
      - "NONE"
      - "TAKE BACK RETURN"
  - name: order_priority
    column: o_orderpriority
    via: lineitem_to_order
    values:
      - "1-URGENT"
      - "2-HIGH"
      - "3-MEDIUM"
      - "4-NOT SPECIFIED"
      - "5-LOW"
  - name: order_status
    column: o_orderstatus
    via: lineitem_to_order
    values:
      - "F"
      - "O"
      - "P"
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
  - name: customer_nation
    column: n_name
    via:
      - lineitem_to_order
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
  - name: manufacturer
    column: p_mfgr
    via: lineitem_to_part
    values:
      - "Manufacturer#1"
      - "Manufacturer#2"
      - "Manufacturer#3"
      - "Manufacturer#4"
      - "Manufacturer#5"
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
  - name: part_type
    column: p_type
    via: lineitem_to_part
  - name: part_container
    column: p_container
    via: lineitem_to_part
  - name: supplier_nation
    column: n_name
    via:
      - lineitem_to_supplier
      - supplier_to_nation
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

Units shipped in the period.
