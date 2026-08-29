//! What a worktree's services are called, and where they are listening.
//!
//! A library rather than two modules inside a binary, and the reason is the second module: a test
//! harness has to be able to LEARN an endpoint, and a binary's modules are reachable from nothing.
//! So [`discovery`] is a library door - the only one - and [`scope`] is beside it because the two
//! answer halves of one question.
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

pub mod discovery;
pub mod scope;
