//! A link whose dial its call gave up on, and which then failed with nothing waiting for it.

use core::num::NonZeroUsize;
use core::time::Duration;

use datafusion_distributed::{ChannelResolver as _, GetWorkerInfoRequest};
use url::Url;

use super::{Links, NodeIdentity};
use crate::pool::WorkingSet;

#[test]
fn a_call_after_one_given_up_on_while_it_dialled_dials_the_node_again() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime builds");
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("a port binds");
        let address = listener.local_addr().expect("a bound port has an address");
        let node = Url::parse(&format!("https://{address}")).expect("a socket address is a URL's authority");
        let links = Links::of(NodeIdentity::signed_for_localhost()).bounded(WorkingSet::of_bytes(NonZeroUsize::MIN));
        let call = || {
            let (links, node) = (links.clone(), node.clone());
            tokio::spawn(async move {
                links
                    .get_worker_client_for_url(&node)
                    .await
                    .expect("a node URL is a link")
                    .get_worker_info(GetWorkerInfoRequest {})
                    .await
                    .map(drop)
            })
        };

        let first = call();
        // Accepted and never answered, so the first call's handshake waits on it.
        let (held, _) = listener.accept().await.expect("the first call dials the node");
        first.abort();
        assert!(
            first.await.is_err_and(|ended| ended.is_cancelled()),
            "the first call ended before it was given up on"
        );
        // The dial the first call gave up on now fails, with no call waiting for it.
        drop(held);

        let second = call();
        let dialled = tokio::time::timeout(Duration::from_secs(10), listener.accept()).await;
        second.abort();
        assert!(
            dialled.is_ok(),
            "the next call was handed how the given-up dial ended, and dialled nothing"
        );
    });
}
