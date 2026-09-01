//! What a worktree's services are called, and where they are listening.
//!
//! A library rather than two modules inside a binary, and the reason is the second module: a test
//! harness has to be able to LEARN an endpoint, and a binary's modules are reachable from nothing.
//! So [`discovery`] is a library door - the only one - and [`scope`] is beside it because the two
//! answer halves of one question.
//!
//! # Two halves, and the second one is what a caller uses
//!
//! * [`discovery`] is the file: publishing it, reading it, and the fact that there is no other way
//!   to learn a port. It is the door that *can* be opened.
//! * [`provisioned`] is the door a caller *should* open. Same file underneath, plus the two things
//!   no test should have to write twice: the diagnostic that names the task to run, and the
//!   skip-or-fail decision from [`requirement`]. A harness that read [`discovery`] directly would
//!   get a connection refused thirty seconds later, blamed on the code under test.
//!
//! Publishing has one door and consumption has one door, and they are not the same door because the
//! two callers are not the same: provisioning knows it is provisioning, while a test does not know
//! whether anything is up.
//!
//! # The split that matters
//!
//! * **Naming is derived** from the worktree path, in [`scope`]. It is stable, readable, and a
//!   collision in it fails loudly at `docker compose up`.
//! * **Ports are allocated**, not derived: published ephemerally so docker and the operating system
//!   pick them, and read back afterwards. [`discovery`] is what reads them back, and there is no
//!   constant to read instead.
//!
//! A hash collision in a NAME is a startup error somebody sees. A hash collision in a PORT is a
//! test that passes against a neighbouring worktree's fixture. That asymmetry is why one of the two
//! is derived and the other is not.
//!
//! **What is NOT here: any knowledge of docker.** Provisioning lives in `xtask`, which is the
//! repo tool and is never packaged. Docker orchestration inside a shipped artifact is test
//! scaffolding delivered to users; `sutura-dev` is not shipped either, but it is the crate a
//! harness links, and a harness has no business being able to start a container.

//! # A third half, and it answers a different question
//!
//! [`issuer`] is not about a provisioned service at all - it is a **mock authorization server in the
//! test sandbox**, behind the default-off `mock-issuer` feature. It is here rather than in the crate
//! that first needed it for the reason [`discovery`] is a library door: leg 1 is verified in the
//! transport, minted-for in a broker and composed in a root, and a fixture living inside one of those
//! three cannot be driven from the other two. What it may never be cited for is written where it is
//! defined, because a venue that cannot state its limit is how *verified* drifts.

pub mod discovery;
#[cfg(feature = "mock-issuer")]
pub mod issuer;
pub mod provisioned;
pub mod requirement;
pub mod scope;
