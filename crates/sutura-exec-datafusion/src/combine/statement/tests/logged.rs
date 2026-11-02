//! What a refused statement writes to the log: that it did not plan at info, and the engine's words,
//! which can name a column or quote a value, at debug only.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sutura_domain::plan::StatementStopped;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber, span};

use super::{Engines, UNBOUNDED, asked};

const COLUMN: &str = "no_such_column";

/// Whether an event naming [`COLUMN`] was written at all, and whether one was written above debug.
#[derive(Default)]
struct Named {
    anywhere: AtomicBool,
    loudly: AtomicBool,
}

struct Listening(Arc<Named>);

/// Whether any field of an event names [`COLUMN`].
struct Naming(bool);

impl Visit for Naming {
    fn record_debug(&mut self, _field: &Field, value: &dyn core::fmt::Debug) {
        self.0 |= format!("{value:?}").contains(COLUMN);
    }
}

impl Subscriber for Listening {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }
    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}
    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut naming = Naming(false);
        event.record(&mut naming);
        if naming.0 {
            self.0.anywhere.store(true, Ordering::SeqCst);
            if *event.metadata().level() < Level::DEBUG {
                self.0.loudly.store(true, Ordering::SeqCst);
            }
        }
    }
    fn enter(&self, _span: &span::Id) {}
    fn exit(&self, _span: &span::Id) {}
}

/// A statement naming a column the source does not have is refused, and the engine's error, which
/// names it, is written at debug and nowhere louder.
#[test]
fn the_engines_words_are_logged_at_debug_only() {
    let named = Arc::new(Named::default());
    let mut legs = Engines::new(None);
    let answer = tracing::subscriber::with_default(Listening(Arc::clone(&named)), || {
        asked(&format!("select {COLUMN} from dim_region"), false, UNBOUNDED, &mut legs)
    });
    assert!(matches!(answer, Some(Err(StatementStopped::NotPlanned))), "{answer:?}");
    assert!(
        named.anywhere.load(Ordering::SeqCst),
        "the engine's words are written at debug"
    );
    assert!(
        !named.loudly.load(Ordering::SeqCst),
        "the engine's words are written above debug"
    );
}
