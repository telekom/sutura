//! The catalog tool over a surface whose bundle is swapped by a refresh.
//!
//! Its own file for the 1000-line cap, like its siblings. What it pins is WHICH bundle the handler
//! reads: one cached when the handler was built passes every other cell here and serves the
//! boot-time catalog to every later caller.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sutura_app::surface::{Adopted, ErasedCause, NotAdopted, Surface, SurfaceFailure};
use sutura_domain::identity::RequestContext;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::raw::{RawOutcome, RawStatement};
use sutura_domain::warehouse::deadline::Deadline;

use super::{connected, describe, text_of};
use crate::testing::{self, ConnectionRefused};

/// Serves `before` until it is handed `after` to adopt, then serves `after` - the swap a refresh
/// makes. Adopting any other bundle is refused, so a cell cannot pass by handing it the wrong one.
/// `Clone` because the transport takes the surface by value and the cell keeps a handle to swap.
#[derive(Clone)]
struct RotatingSurface {
    before: Arc<PinnedDefinitions>,
    after: Arc<PinnedDefinitions>,
    rotated: Arc<AtomicBool>,
}

impl Surface for RotatingSurface {
    fn definitions(&self) -> Arc<PinnedDefinitions> {
        Arc::clone(if self.rotated.load(Ordering::SeqCst) {
            &self.after
        } else {
            &self.before
        })
    }

    fn adopt(&self, next: PinnedDefinitions) -> Result<Adopted, NotAdopted> {
        if next.digest() != self.after.digest() {
            return Err(NotAdopted::Preflight {
                cause: ErasedCause::from("this fixture adopts only the bundle it was built to rotate to"),
            });
        }
        let previous = self.definitions().digest().clone();
        self.rotated.store(true, Ordering::SeqCst);
        Ok(Adopted::Rotated {
            previous,
            digest: self.after.digest().clone(),
        })
    }

    fn answer(&self, _: &RequestContext, _: &Query, _: Deadline) -> Result<ToolOutcome, SurfaceFailure> {
        Err(SurfaceFailure::Warehouse {
            cause: Box::new(ConnectionRefused),
        })
    }

    fn run_sql(&self, _: &RequestContext, _: &RawStatement, _: Deadline) -> Result<RawOutcome, SurfaceFailure> {
        Err(SurfaceFailure::Warehouse {
            cause: Box::new(ConnectionRefused),
        })
    }

    fn spend_headroom_bytes(&self) -> Option<u64> {
        None
    }

    fn spent_bytes_total(&self) -> Option<u64> {
        None
    }
}

fn rotating() -> (RotatingSurface, PinnedDefinitions) {
    let after = testing::described_bundle("Revenue, restated.", "Sales region.");
    let surface = RotatingSurface {
        before: Arc::new(testing::bundle()),
        after: Arc::new(after.clone()),
        rotated: Arc::new(AtomicBool::new(false)),
    };
    (surface, after)
}

#[tokio::test]
async fn a_catalog_call_after_a_refresh_reads_the_bundle_now_being_served() {
    let (surface, after) = rotating();
    let client = connected(surface.clone()).await;

    let before = text_of(&client.call_tool(describe()).await.expect("the catalog tool answers"));
    assert!(before.contains("Revenue, in minor units."), "{before}");

    let adopted = surface.adopt(after).expect("the fixture adopts the bundle it rotates to");
    assert!(matches!(adopted, Adopted::Rotated { .. }), "{adopted:?}");

    let now = text_of(&client.call_tool(describe()).await.expect("the catalog tool answers"));
    assert!(now.contains("Revenue, restated."), "{now}");
    assert!(!now.contains("Revenue, in minor units."), "{now}");
    drop(client.cancel().await);
}
