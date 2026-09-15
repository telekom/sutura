//! The reader threads and the channel they feed, as one guard whose log cannot be read early.
//!
//! **A child module of the harness rather than a type inside it, and privacy is the whole reason.**
//! `github.com/telekom/sutura#415`: the fix for `#387` put a join in front of the collection at
//! both call sites, and the test that arrived with it pinned the *helper* doing the joining. Measured
//! on that head: reverting [`super::refused_to_start`] to `drop(readers)` plus a `try_recv` sweep,
//! leaving the helper and its test intact, left the suite **10 passed; 0 failed**. So the site the
//! issue was about had no mechanism at all, while the other one had `-D dead_code` behind it by
//! accident.
//!
//! A test cannot close that gap here. What separates a joining collection from a sweeping one is an
//! injected delay, and the delay on the refusing path is the process's own scheduling: the wait loop
//! polls at 25ms, so the readers have almost always forwarded the refusal before the child is
//! observed to have exited - the rate `#387` was measured at is **29 of 3000 (0.97%)**, and a
//! deterministic test would have to slow reader threads it does not own.
//!
//! So the guard is a type instead. Both halves live in one struct with **private** fields, and Rust
//! does not let a parent module reach into a child's privates - so `super` can obtain the log only by
//! consuming the guard, which joins first. The exact revert measured above no longer compiles.
//!
//! **The limit, next to the claim.** This holds against a *revert*, not against a rewrite: nothing
//! stops a future author re-creating a `channel()` and two `spawn`s inside `refused_to_start` and
//! sweeping that. That is the same bar `Served::readers`' `dead_code` guard clears - deleting the
//! field's only reader is refused, deleting the field is not - and it is why
//! [`Reading::of_readers`] takes the receiver **by value**: a caller that hands its channel over
//! cannot also keep a sweep of it.

use std::process::Child;
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;

use super::{forward, joined};

/// A spawned process's two output streams, being read.
///
/// The channel is created in [`Reading::of`] and never handed out, so the only way to reach a line is
/// [`Reading::finished`], which joins.
pub(crate) struct Reading {
    /// The threads feeding `lines`. Read only by the join in `finished`.
    readers: Vec<JoinHandle<()>>,
    /// Where both streams arrive. **Private on purpose** - see this module's own header.
    lines: Receiver<String>,
}

impl Reading {
    /// Takes both piped streams off `child` and starts reading them.
    ///
    /// Takes the child rather than the two streams, so a caller never holds the halves and cannot
    /// spawn a third reader over one of them.
    pub(crate) fn of(child: &mut Child) -> Self {
        let stdout = child.stdout.take().expect("standard output was piped");
        let stderr = child.stderr.take().expect("standard error was piped");
        let (sender, lines) = channel();
        let second = sender.clone();
        Self {
            readers: vec![
                std::thread::spawn(move || forward(stdout, &sender)),
                std::thread::spawn(move || forward(stderr, &second)),
            ],
            lines,
        }
    }

    /// The same over reader handles and a channel the caller already has.
    ///
    /// Exists for the one test that asserts the ORDER inside [`Reading::finished`], which needs a
    /// reader it can delay and therefore cannot come from [`Reading::of`]. The receiver is taken by
    /// value, so this is not a way to hold a sweep alongside a guard.
    pub(crate) fn of_readers(readers: Vec<JoinHandle<()>>, lines: Receiver<String>) -> Self {
        Self { readers, lines }
    }

    /// Everything the process wrote, collected once both readers have returned.
    ///
    /// **Consuming, and the order is the whole property.** `try_recv` is non-blocking and stops at
    /// the first empty channel, so draining BEFORE the join returns the log minus whatever was still
    /// in flight - and a test asserting on a refusal's own sentence then fails as *the deployment did
    /// not refuse*, which is the one diagnosis nobody should be given wrongly. That is
    /// `github.com/telekom/sutura#387`.
    pub(crate) fn finished(self) -> Vec<String> {
        joined(self.readers);
        let mut said = Vec::new();
        while let Ok(line) = self.lines.try_recv() {
            said.push(line);
        }
        said
    }
}
