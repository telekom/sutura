# TPC-H data generated inside the Nix sandbox, by the pinned DuckDB with its in-tree `tpch` extension
# linked into the CLI.
#
# **No download.** `INSTALL tpch` fetches a prebuilt extension for the engine's version, and a
# build that names itself by a git describe has none published. DuckDB's own source carries
# `extension/tpch` and lists it in `.github/config/core.cmake`, which `nix/duckdb.nix`'s build
# already points `DUCKDB_EXTENSION_CONFIGS` at: it is built, but only an extension that is also
# listed in `STATICALLY_LINK_EXTENSIONS` is linked into the CLI. So this rebuilds the same DuckDB with
# `tpch` added to that list, and `LOAD tpch` resolves it from the binary.
#
# **The shipped libduckdb is not touched.** `nix/duckdb.nix` keeps its derivation and its
# `lib` output; this is a separate CLI whose only consumer is the generator below.
#
# **`tpchSf scale` is one data set, gated at build time.** It holds `star.duckdb` (all seven tables
# a star needs; `partsupp` is dropped, no question reads it), `dimensions/<table>.csv` for a
# PostgreSQL copy of the five dimension tables, and `counts.json`. The build FAILS unless every
# table's row count, read back from the finished file, equals the TPC-H specification at that scale,
# and every CSV holds the same number of rows. `lineitem` is not a multiple of the scale, so a scale
# without a published count is refused at evaluation rather than generated unchecked.
#
# **Cross keys are BIGINT**, the type a PostgreSQL key has, so a federated join's two columns agree
# whatever integer width `dbgen` chose. The dimension tables keep their generated types in DuckDB.
#
# Limit: the gate proves the row counts and that the CSVs match them. It does not prove the values
# are the specification's, which `dbgen`'s own seeding decides.
{ pkgs }:
let
  inherit (pkgs) lib;

  base = (import ./duckdb.nix { inherit pkgs; }).package;

  cli = base.overrideAttrs (old: {
    pname = "duckdb-tpch";
    cmakeFlags = old.cmakeFlags ++ [
      (lib.cmakeFeature "STATICALLY_LINK_EXTENSIONS" "core_functions;parquet;json;icu;tpch")
    ];
    doInstallCheck = false;
  });

  dimensions = {
    customer = [ "c_custkey" "c_nationkey" "c_mktsegment" ];
    part = [ "p_partkey" "p_mfgr" "p_brand" "p_type" "p_container" ];
    supplier = [ "s_suppkey" "s_nationkey" ];
    nation = [ "n_nationkey" "n_name" "n_regionkey" ];
    region = [ "r_regionkey" "r_name" ];
  };

  crossKeys = [
    { table = "lineitem"; column = "l_partkey"; }
    { table = "lineitem"; column = "l_suppkey"; }
    { table = "orders"; column = "o_custkey"; }
  ];

  tables = [ "lineitem" "orders" ] ++ builtins.attrNames dimensions;

  # The `lineitem` row count the specification publishes per scale factor.
  lineitemRows = {
    "1" = 6001215;
    "10" = 59986052;
  };

  expectedCounts =
    scale:
    assert lib.assertMsg (lib.isInt scale && scale > 0) "tpch-data: scale must be a positive integer";
    assert lib.assertMsg (lineitemRows ? ${toString scale})
      "tpch-data: no published lineitem count for SF${toString scale}; add it to lineitemRows";
    {
      lineitem = lineitemRows.${toString scale};
      orders = 1500000 * scale;
      customer = 150000 * scale;
      part = 200000 * scale;
      supplier = 10000 * scale;
      nation = 25;
      region = 5;
    };

  generate =
    scale:
    pkgs.writeText "tpch-sf${toString scale}-generate.sql" ''
      LOAD tpch;
      CALL dbgen(sf = ${toString scale});
      ${lib.concatMapStringsSep "\n" (
        { table, column }: "ALTER TABLE ${table} ALTER ${column} SET DATA TYPE BIGINT;"
      ) crossKeys}
      ${lib.concatStringsSep "\n" (
        lib.mapAttrsToList (
          table: columns:
          "COPY (SELECT ${lib.concatStringsSep ", " columns} FROM ${table}) TO 'dimensions/${table}.csv' (HEADER);"
        ) dimensions
      )}
      DROP TABLE partsupp;
      CHECKPOINT;
    '';

  count = pkgs.writeText "tpch-count.sql" ''
    SELECT ${lib.concatMapStringsSep ", " (t: "(SELECT count(*) FROM ${t}) AS ${t}") tables};
  '';

  tpchSf =
    scale:
    let
      expected = pkgs.writeText "tpch-sf${toString scale}-expected.json" (
        builtins.toJSON (expectedCounts scale)
      );
    in
    pkgs.runCommand "tpch-sf${toString scale}"
      {
        nativeBuildInputs = [
          cli
          pkgs.jq
        ];
        passthru = { inherit scale; };
      }
      ''
        export HOME="$TMPDIR"
        mkdir -p "$out/dimensions"
        cd "$out"

        duckdb star.duckdb < ${generate scale}
        test ! -e star.duckdb.wal

        duckdb -readonly -json star.duckdb < ${count} | jq -e '.[0]' > "$TMPDIR/counts"
        jq -n --argjson scale ${toString scale} --slurpfile counts "$TMPDIR/counts" \
          '{scale: $scale, counts: $counts[0]}' > counts.json

        fail() {
          echo "tpch-sf${toString scale}: $1" >&2
          exit 1
        }

        jq -e --slurpfile want ${expected} '.counts == $want[0]' counts.json > /dev/null \
          || { jq -c --slurpfile want ${expected} '{want: $want[0], got: .counts}' counts.json >&2
               fail "row counts differ from the specification"; }

        for table in ${lib.concatStringsSep " " (builtins.attrNames dimensions)}; do
          rows=$(( $(wc -l < "dimensions/$table.csv") - 1 ))
          want=$(jq -r --arg table "$table" '.counts[$table]' counts.json)
          [ "$rows" = "$want" ] || fail "dimensions/$table.csv holds $rows rows, star.duckdb holds $want"
        done
      '';
in
{
  inherit cli tpchSf;
}
