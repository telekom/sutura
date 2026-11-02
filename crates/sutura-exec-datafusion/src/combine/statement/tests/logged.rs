//! What a refused statement writes to the log: that it did not plan at info, and the engine's words,
//! which can name a column or quote a value, at debug only.

use std::sync::mpsc::{Sender, channel};

use sutura_domain::plan::StatementStopped;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber, span};

use super::{Engines, UNBOUNDED, asked};

const COLUMN: &str = "no_such_column";

/// Each event written on this thread while `during` ran: its level and its fields.
pub(super) type Written = Vec<(Level, String)>;

/// What `during` returned, beside what it wrote.
pub(super) fn recorded<T>(during: impl FnOnce() -> T) -> (T, Written) {
    let (sender, written) = channel();
    let answer = tracing::subscriber::with_default(Recording(sender), during);
    (answer, written.try_iter().collect())
}

struct Recording(Sender<(Level, String)>);

/// An event's fields, each as `name=value`.
struct Fields(Vec<String>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
        self.0.push(format!("{}={value:?}", field.name()));
    }
}

impl Subscriber for Recording {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }
    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}
    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields(Vec::new());
        event.record(&mut fields);
        drop(self.0.send((*event.metadata().level(), fields.0.join(" "))));
    }
    fn enter(&self, _span: &span::Id) {}
    fn exit(&self, _span: &span::Id) {}
}

/// A statement naming a column the source does not have is refused, and the engine's error, which
/// names it, is written at debug and nowhere louder.
#[test]
fn the_engines_words_are_logged_at_debug_only() {
    let mut legs = Engines::new(None);
    let (answer, written) = recorded(|| asked(&format!("select {COLUMN} from dim_region"), false, UNBOUNDED, &mut legs));
    assert!(matches!(answer, Some(Err(StatementStopped::NotPlanned))), "{answer:?}");
    let naming = || written.iter().filter(|(_, fields)| fields.contains(COLUMN));
    assert!(
        naming().next().is_some(),
        "the engine's words are written at debug: {written:?}"
    );
    assert!(
        naming().all(|(level, _)| *level >= Level::DEBUG),
        "the engine's words are written above debug: {written:?}"
    );
}
