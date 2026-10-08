//! The address a composition root resolves out of one PostgreSQL declaration, before a
//! [`Conninfo`](crate::Conninfo) is built.

use std::path::Path;

/// The address a PostgreSQL source is dialled through.
#[derive(Clone, Copy)]
pub enum ConnectionTarget<'a> {
    /// A TCP host name or address.
    Host(&'a str),
    /// A unix socket directory.
    UnixSocket(&'a Path),
}
