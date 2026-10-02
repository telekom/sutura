//! Runs one test body under a proxy named by the ENVIRONMENT, and fails it if the proxy is dialled.
//!
//! A test cannot set a proxy variable in its own process - `std::env::set_var` is `unsafe` in Rust
//! 2024 and every crate here forbids `unsafe` - and an agent reads the variables when it is built,
//! inside the constructor under test. So [`dialled_directly`] re-runs the calling test in a child
//! copy of its own test binary, with every proxy variable naming a listener this process holds and
//! `NO_PROXY` removed, and the child runs the body. The parent fails the test the moment that
//! listener accepts a connection, or if the child's run did not pass exactly one test.
//!
//! Limit: a child that passes must have reached its own target, which only the body can assert -
//! a body that sends nothing passes here too.

use std::io::ErrorKind;
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Set in the child, so the same test function runs the body there and the harness here.
const CHILD: &str = "SUTURA_DEV_UNDER_ENV_PROXY";

/// Every variable `ureq` reads a proxy from, in both cases.
const PROXY_VARIABLES: [&str; 6] = [
    "ALL_PROXY",
    "all_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
];

/// Long enough for a loaded host to start a test binary; a child still running then is a red.
const CHILD_BUDGET: Duration = Duration::from_secs(60);

/// Runs `body` in a child copy of the calling test, under an environment proxy, and panics if the
/// proxy is dialled or the child does not pass.
///
/// `module_path` is the caller's `module_path!()` and `test` its function name - together the
/// name libtest runs it by.
///
/// # Panics
///
/// On a dialled proxy, a child that did not pass exactly one test, or a harness that cannot start
/// one.
#[expect(
    clippy::panic,
    clippy::expect_used,
    reason = "a test harness: its panics are the test's own assertions"
)]
pub fn dialled_directly(module_path: &str, test: &str, body: impl FnOnce()) {
    if std::env::var_os(CHILD).is_some() {
        body();
        return;
    }
    let proxy = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
    proxy.set_nonblocking(true).expect("the proxy listener can be polled");
    let url = format!("http://{}", proxy.local_addr().expect("a bound listener has an address"));
    let name = module_path
        .split_once("::")
        .map_or_else(|| test.to_owned(), |(_, inner)| format!("{inner}::{test}"));
    let mut command = Command::new(std::env::current_exe().expect("a test binary knows its own path"));
    command
        .args([name.as_str(), "--exact", "--test-threads=1"])
        .env(CHILD, "1")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for variable in PROXY_VARIABLES {
        command.env(variable, &url);
    }
    let mut child = command.spawn().expect("the test binary re-runs itself");
    let started = Instant::now();
    loop {
        match proxy.accept() {
            Ok(_) => {
                drop(child.kill());
                panic!("`{name}` dialled the proxy its environment names");
            }
            Err(cause) if cause.kind() == ErrorKind::WouldBlock => {}
            Err(cause) => panic!("the proxy listener failed: {cause}"),
        }
        if child.try_wait().expect("the child can be polled").is_some() {
            break;
        }
        if started.elapsed() > CHILD_BUDGET {
            drop(child.kill());
            panic!("`{name}` did not finish under an environment proxy within {CHILD_BUDGET:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(proxy.accept().is_err(), "`{name}` dialled the proxy its environment names");
    let output = child.wait_with_output().expect("the child's output is readable");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains(" 1 passed;"),
        "`{name}` did not pass alone under an environment proxy:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
