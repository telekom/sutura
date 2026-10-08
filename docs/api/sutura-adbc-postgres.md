<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-adbc-postgres

The public API of `sutura-adbc-postgres`, rendered from rustdoc JSON.

The PostgreSQL ADBC connector: the libpq connection string and its TLS posture (`Conninfo`,
`Channel`), the driver a process loads (`PostgresDriver`), and the one connection opened
over them (`PostgresDriver::connect`).

It is shared rather than adapter code so that `sutura-exec-postgres` and a metadata reader dial
a source through one refusal set. It names no SQL dialect - a catalog adapter may not reach
`sutura-sql` (`xtask/src/boundaries/edges.rs`), and neither may this crate - and it carries no
adapter prefix, so its normal tree is held off adapters, the application, settings and the
transports by its own row in `xtask/src/boundaries/shared_client.rs`.

## `use Channel`

How the channel to the source is secured, as the composition root resolved the declaration.

## `use Conninfo`

The connection string for one source. Only `Conninfo::new` and `Conninfo::kerberos` make
one, and its `Debug` is the `Secret`'s, so the password it carries is never printed.

## `use GssEncryption`

Whether GSSAPI encrypts the channel - libpq's `gssencmode`.

## `use InvalidKerberosService`

A declared Kerberos service name that is not one.

## `use Kerberos`

A Kerberos sign-in through GSSAPI, as the declaration names it.

## `use KerberosService`

The service half of the server's principal, `<service>/<host>` - libpq's `krbsrvname`.

## `use UnusableChannel`

A declared connection the ADBC transport cannot hold to, refused before anything dials.

## `use AdbcError`

Why a PostgreSQL ADBC call could not answer.

## `use MOUNTED_DRIVER`

The variable a host that links no driver names a mounted one with.

**Not a settings key**: which driver file a host carries is a property of the host rather than of
the semantic deployment, and a release artefact that links one never reads it - the order
`bigquery_driver` in `sutura-cli` gives for the other ADBC adapter, for its reason: a mounted
path must not be able to displace the driver a published artefact carries.

## `use NoDriver`

Why this process has no PostgreSQL driver to open.

## `use PostgresDriver`

Where the PostgreSQL driver comes from: this artefact's own link, or a mounted `.so`.

Not `sutura_adbc::DriverLocation`, whose linked route is the `BigQuery` archive; a mounted path is
parsed by it, so an empty or relative one is refused exactly as for every ADBC adapter.

## `use UnusableDriverPath`

## `use ConnectionTarget`

The address a PostgreSQL source is dialled through.
