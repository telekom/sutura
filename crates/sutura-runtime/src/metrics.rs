//! A hand-rolled Prometheus registry, and the one findable thing about it is the shape of its
//! labels.
//!
//! # Why this is not a crate
//!
//! `docs/adr/0015` Decision 3 prices it: a facade crate arrives with a global recorder, a macro
//! layer and a label API typed as `String` - which is precisely the cardinality hole this module
//! exists to close. The Prometheus text environment format is stable and line-oriented; every gauge
//! here is an atomic load; the histograms are a fixed bucket array and cumulative counters. Writing
//! the boilerplate by hand costs nothing and adds no dependency, which is the whole of the decision.
//!
//! # A label carries only process-lifetime text
//!
//! Every labeled registration and update below takes [`Label`], whose ordinary constructor accepts
//! only `&'static str`. The transport supplies literals and its own fixed code vocabularies, so
//! request-owned or request-borrowed text cannot flow directly into a label. Registration also
//! supplies the complete key set at boot; an update for any other label is ignored rather than
//! creating a series. The companion behavioural test pins that pre-registered set exactly.
//!
//! # The limit no type here reaches
//!
//! Rust's lifetime is not proof that bytes originated in the binary: production code could
//! deliberately turn request text into `&'static str` with `Box::leak`. What prevents that text
//! minting a series even then is the registry's closed pre-registered key set. Neither mechanism
//! bounds the product of label dimensions an author chooses; the exact-series test catches that,
//! and there is no `check-boundaries`-style gate for construction sites.
//!
//! # The memory series, and their limit
//!
//! The three engine-pool series - reserved bytes, the limit, and refusals - are **not shipped at
//! all**: `docs/adr/0015` specifies them and keeps them absent rather than zero (the pool bounds
//! the engine's own operators and nothing else). There is no `Option` gauge and no live closure,
//! because the exact-series snapshot test means an author who registers one would be trading an
//! asserted export for a silent gap. The transport module records the same absence in its own doc.
//!
//! # A scrape must not make the service work
//!
//! [`Registry::render`] reads atomics and fixed strings only. The registry holds no `Surface`, no
//! catalog, no engine and no data source - the design `docs/adr/0015` states and the metrics route's
//! state type enforces. `render` takes **no lock of any kind**: registration happens through a
//! [`RegistryBuilder`], and [`RegistryBuilder::build`] freezes the series into an immutable [`Vec`]
//! before the registry is shared.

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A monotonic counter, shared by `Arc` between a handle and the registry.
#[derive(Debug, Clone, Default)]
pub struct Counter(Arc<AtomicU64>);

impl Counter {
    /// Adds one.
    pub fn inc(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    /// Adds `n`.
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    /// The current value.
    #[must_use]
    pub fn value(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// A gauge, shared by `Arc` between a handle and the registry.
#[derive(Debug, Clone, Default)]
pub struct Gauge(Arc<AtomicU64>);

impl Gauge {
    /// Sets a value.
    pub fn set(&self, n: u64) {
        self.0.store(n, Ordering::Relaxed);
    }

    /// Adjusts by `delta`, keeping the value from underflowing zero.
    ///
    /// A compare-and-exchange loop rather than a single `try_update`: a `try_update` whose closure
    /// sees contention reports that contention to a caller who would have to decide what to do with
    /// it, and discarding that `Result` is exactly the silent update-loss this loop is written to
    /// rule out. Here the loop re-reads and retries until an update lands, and the clamp to zero is
    /// `saturating_add_signed` - a number, not a silently-inspected `unwrap_or`.
    pub fn adjust(&self, delta: i64) {
        let mut current = self.0.load(Ordering::Relaxed);
        loop {
            let next = current.saturating_add_signed(delta);
            match self
                .0
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// The current value.
    #[must_use]
    pub fn value(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// A fractional sum, shared by `Arc` between a histogram's handle and the registry.
///
/// The Prometheus histogram `_sum` is a float - a duration is fractional, and a histogram over
/// durations must not round it to integer seconds or the sum stops meaning anything. The float is
/// stored as its bit pattern in an `AtomicU64` and accumulated with a compare-and-exchange loop, so
/// every observation lands and fractional parts survive.
#[derive(Debug, Clone, Default)]
pub struct FloatSum(Arc<AtomicU64>);

impl FloatSum {
    /// Adds `value`, which must be finite and non-negative.
    #[expect(
        clippy::float_arithmetic,
        reason = "the histogram `_sum` is a real-valued total that keeps its fractional part, so the \
                 accumulation is float addition by design"
    )]
    pub fn add(&self, value: f64) {
        let mut current = self.0.load(Ordering::Relaxed);
        loop {
            let next = (f64::from_bits(current) + value).to_bits();
            match self
                .0
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// The current sum.
    #[must_use]
    pub fn value(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// One process-lifetime label value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label(&'static str);

impl Label {
    /// The value, for rendering and for a key lookup.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Builds a [`Label`] from a literal.
///
/// # Request text does not flow directly into a label
///
/// The parameter is `&'static str`, so a value a request produced cannot be one:
///
/// ```compile_fail
/// let from_a_request = String::from("metric_unknown");
/// let _ = sutura_runtime::metrics::label(&from_a_request);
/// ```
///
/// And the twin that must compile, so the snippet above is a statement about the lifetime rather
/// than a broken example:
///
/// ```
/// assert_eq!(sutura_runtime::metrics::label("metric_unknown").as_str(), "metric_unknown");
/// ```
#[must_use]
pub const fn label(value: &'static str) -> Label {
    Label(value)
}

#[derive(Debug)]
enum Series {
    /// A bare counter.
    Counter {
        name: &'static str,
        value: Counter,
    },
    Gauge {
        name: &'static str,
        value: Gauge,
    },
    /// A counter with one closed label set.
    LabeledCounter {
        name: &'static str,
        label: &'static str,
        values: Vec<(Label, Counter)>,
    },
    /// A gauge with one closed label set.
    LabeledGauge {
        name: &'static str,
        label: &'static str,
        values: Vec<(Label, Gauge)>,
    },
    /// A histogram: fixed ascending bounds and cumulative counts, plus sum and count.
    Histogram {
        name: &'static str,
        bounds: &'static [f64],
        counts: Vec<Counter>,
        sum: FloatSum,
        count: Counter,
    },
}

impl Series {
    fn render(&self, out: &mut String) -> std::fmt::Result {
        match self {
            Self::Counter { name, value } => {
                writeln!(out, "# TYPE {name} counter")?;
                writeln!(out, "{name} {}", value.value())
            }
            Self::Gauge { name, value } => {
                writeln!(out, "# TYPE {name} gauge")?;
                writeln!(out, "{name} {}", value.value())
            }
            Self::LabeledCounter { name, label, values } => {
                writeln!(out, "# TYPE {name} counter")?;
                for (key, value) in values {
                    writeln!(out, "{name}{{{label}=\"{}\"}} {}", key.as_str(), value.value())?;
                }
                Ok(())
            }
            Self::LabeledGauge { name, label, values } => {
                writeln!(out, "# TYPE {name} gauge")?;
                for (key, value) in values {
                    writeln!(out, "{name}{{{label}=\"{}\"}} {}", key.as_str(), value.value())?;
                }
                Ok(())
            }
            Self::Histogram {
                name,
                bounds,
                counts,
                sum,
                count,
            } => {
                writeln!(out, "# TYPE {name} histogram")?;
                let mut cumulative = 0_u64;
                // `counts` and `bounds` are built to the same length (see
                // `RegistryBuilder::histogram`), so zipping the two can never drop a live bucket.
                for (bucket, bound) in counts.iter().zip(bounds.iter()) {
                    cumulative = cumulative.saturating_add(bucket.value());
                    writeln!(out, "{name}_bucket{{le=\"{bound}\"}} {cumulative}")?;
                }
                writeln!(out, "{name}_bucket{{le=\"+Inf\"}} {}", count.value())?;
                writeln!(out, "{name}_sum {}", sum.value())?;
                writeln!(out, "{name}_count {}", count.value())
            }
        }
    }
}

/// Collects every series at boot, and freezes them into an immutable [`Registry`].
///
/// `docs/adr/0015`'s scrape contract - **a scrape takes no request-path lock and registration is
/// boot-only** - is held by shape rather than by convention. Registration the registry hands out
/// after `build` would need a `&mut` into shared state or a lock; `Registry` has neither, so the
/// only way to add a series is through this builder, and once `build` has run the set is closed.
///
/// The caller owns the builder until it consumes it. The type is neither `Clone` nor `Copy`, so
/// one builder cannot produce two independently registered views by accident. This does not make
/// the registry process-global: two builders can deliberately create two registries.
#[derive(Debug, Default)]
pub struct RegistryBuilder {
    series: Vec<Series>,
}

impl RegistryBuilder {
    /// Registers a counter and returns the handle.
    #[must_use]
    pub fn counter(&mut self, name: &'static str) -> Counter {
        let counter = Counter::default();
        self.series.push(Series::Counter {
            name,
            value: counter.clone(),
        });
        counter
    }

    /// Registers a gauge and returns the handle.
    #[must_use]
    pub fn gauge(&mut self, name: &'static str) -> Gauge {
        let gauge = Gauge::default();
        self.series.push(Series::Gauge {
            name,
            value: gauge.clone(),
        });
        gauge
    }

    /// Registers a labeled counter (closed literal keys) and returns the handle.
    #[must_use]
    pub fn labeled_counter(&mut self, name: &'static str, label: &'static str, keys: &'static [Label]) -> LabeledCounter {
        let values: Vec<(Label, Counter)> = keys.iter().map(|key| (*key, Counter::default())).collect();
        self.series.push(Series::LabeledCounter {
            name,
            label,
            values: values.clone(),
        });
        LabeledCounter { values }
    }

    /// Registers a labeled gauge (closed literal keys) and returns the handle.
    #[must_use]
    pub fn labeled_gauge(&mut self, name: &'static str, label: &'static str, keys: &'static [Label]) -> LabeledGauge {
        let values: Vec<(Label, Gauge)> = keys.iter().map(|key| (*key, Gauge::default())).collect();
        self.series.push(Series::LabeledGauge {
            name,
            label,
            values: values.clone(),
        });
        LabeledGauge { values }
    }

    /// Registers a histogram with ascending upper bounds and returns the handle.
    #[must_use]
    pub fn histogram(&mut self, name: &'static str, bounds: &'static [f64]) -> Histogram {
        let counts: Vec<Counter> = bounds.iter().map(|_| Counter::default()).collect();
        let sum = FloatSum::default();
        let count = Counter::default();
        self.series.push(Series::Histogram {
            name,
            bounds,
            counts: counts.clone(),
            sum: sum.clone(),
            count: count.clone(),
        });
        Histogram {
            bounds,
            counts,
            sum,
            count,
        }
    }

    /// Freezes the collected series into a [`Registry`] that only render (and a reader) touches.
    #[must_use]
    pub fn build(self) -> Registry {
        Registry { series: self.series }
    }
}

/// The registry: every series a deployment exports, held in one place.
///
/// Built via [`RegistryBuilder::build`] and handed to observers and a renderer. Handles share their
/// atomics with the registry by `Arc`, so a handle and the registry it came from read the same
/// value. The type does not enforce one registry per process; that ownership belongs to its caller.
///
/// **Immutable after [`RegistryBuilder::build`]:** the series set is a plain [`Vec`], closed at
/// boot, so [`Self::render`] reads atomics and fixed strings and takes no lock and registers
/// nothing.
pub struct Registry {
    series: Vec<Series>,
}

impl core::fmt::Debug for Registry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Registry").field("series", &self.series.len()).finish()
    }
}

impl Registry {
    /// An empty registry.
    ///
    /// For a test that renders a single series family in isolation, and for the `Default` shape an
    /// immutable set takes. It registers nothing and cannot have series added to it - that is the
    /// builder's, and only its.
    #[must_use]
    pub const fn new() -> Self {
        Self { series: Vec::new() }
    }

    /// Every series, in Text Exposition format, one family per section.
    ///
    /// **The whole point of the registry.** `O(series)` and constant-time per series - atomic loads
    /// and fixed strings only. It takes no lock the request path holds - it takes no lock at all.
    /// It acquires no admission permit, and reaches no other crate's state.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for entry in &self.series {
            // Writing to a String cannot fail: `entry.render` returns a `fmt::Result` only so the
            // compiler can prove the write propagates, and `String`'s `Write` never produces an
            // error. This one `expect` is on an infallible built-in, not on caller input. It is an
            // `#[expect]` because every other way to name the `Result` trips a stricter lint: a
            // bare drop is a non-drop, `let _ = ` is an ignored `must_use`, and neither is the
            // honest "this cannot happen".
            #[expect(clippy::expect_used, reason = "writing to a String cannot fail")]
            entry.render(&mut out).expect("writing to a String cannot fail");
        }
        out
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// A handle to a labeled counter family, sharing the registry's atomics.
#[derive(Debug, Clone)]
pub struct LabeledCounter {
    values: Vec<(Label, Counter)>,
}

impl LabeledCounter {
    /// Increments the series for a closed key.
    ///
    /// An unknown key is ignored: the registry only ever renders keys it was built with, so a caller
    /// asking for an unknown key is a defect rather than a way to mint a label.
    pub fn inc(&self, key: Label) {
        if let Some((_, counter)) = self.values.iter().find(|(k, _)| *k == key) {
            counter.inc();
        }
    }
}

/// A handle to a labeled gauge family.
#[derive(Debug, Clone)]
pub struct LabeledGauge {
    values: Vec<(Label, Gauge)>,
}

impl LabeledGauge {
    /// Sets the series for a closed key.
    pub fn set(&self, key: Label, value: u64) {
        if let Some((_, gauge)) = self.values.iter().find(|(k, _)| *k == key) {
            gauge.set(value);
        }
    }
}

/// A handle to a histogram, sharing the registry's buckets.
#[derive(Debug, Clone)]
pub struct Histogram {
    bounds: &'static [f64],
    counts: Vec<Counter>,
    sum: FloatSum,
    count: Counter,
}

impl Histogram {
    /// Records one observation (a duration or a row count, as `f64`).
    ///
    /// Each bucket below stores the count of observations *in that bound's exclusive range*; the
    /// renderer turns those into cumulative counts. A value lands in exactly the first bucket whose
    /// upper bound it meets. The sum keeps its fractional part: it is a float, not a count rounded
    /// to integers.
    pub fn observe(&self, value: f64) {
        let value = value.max(0.0);
        self.sum.add(value);
        self.count.inc();
        // `counts` and `bounds` are built to the same length (see `RegistryBuilder::histogram`), so
        // the zip never drops a live bucket.
        for (bucket, bound) in self.counts.iter().zip(self.bounds.iter()) {
            if value <= *bound {
                bucket.inc();
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FloatSum, Registry, RegistryBuilder, label};

    /// The bits bit-pattern arithmetic must round-trip through the atomic for.
    #[test]
    fn a_float_sum_accumulates_fractional_values() {
        let sum = FloatSum::default();
        sum.add(0.05);
        sum.add(5.0);
        sum.add(20.0);
        assert_eq!(sum.value(), 25.05);
    }

    #[test]
    fn a_float_sum_rounds_trips_negative_zero() {
        let sum = FloatSum::default();
        sum.add(-0.0);
        assert_eq!(sum.value(), 0.0);
    }

    #[test]
    fn a_counter_increments_and_renders() {
        let mut builder = RegistryBuilder::default();
        let counter = builder.counter("sutura_questions_total");
        assert_eq!(counter.value(), 0);
        counter.inc();
        assert_eq!(counter.value(), 1);
        let rendered = builder.build().render();
        assert!(rendered.contains("# TYPE sutura_questions_total counter"));
        assert!(rendered.contains("sutura_questions_total 1"));
    }

    #[test]
    fn a_gauge_sets_and_adjusts_without_underflow() {
        let mut builder = RegistryBuilder::default();
        let gauge = builder.gauge("sutura_execution_slots_in_use");
        gauge.set(1);
        gauge.adjust(-2); // clamps to zero
        assert_eq!(gauge.value(), 0);
        // And a concurrent-style interleave leaves no update silently lost: two adjustments to a
        // contended-looking value still both land.
        gauge.set(0);
        gauge.adjust(3);
        gauge.adjust(1);
        assert_eq!(gauge.value(), 4);
    }

    #[test]
    fn a_labeled_counter_only_renders_and_counts_its_closed_keys() {
        const KEYS: &[super::Label] = &[label("ok"), label("refused")];
        let mut builder = RegistryBuilder::default();
        let counter = builder.labeled_counter("sutura_questions_total", "code", KEYS);
        counter.inc(label("refused"));
        counter.inc(label("something_else")); // not a declared key - ignored, not a new label
        let rendered = builder.build().render();
        assert!(rendered.contains("sutura_questions_total{code=\"ok\"} 0"));
        assert!(rendered.contains("sutura_questions_total{code=\"refused\"} 1"));
        assert!(!rendered.contains("something_else"));
    }

    #[test]
    fn a_labeled_gauge_only_renders_its_closed_keys() {
        const KEYS: &[super::Label] = &[label("probe"), label("general")];
        let mut builder = RegistryBuilder::default();
        let gauge = builder.labeled_gauge("sutura_rate_limit_buckets", "tier", KEYS);
        gauge.set(label("general"), 7);
        gauge.set(label("something_else"), 9);
        let rendered = builder.build().render();
        assert!(rendered.contains("sutura_rate_limit_buckets{tier=\"probe\"} 0"));
        assert!(rendered.contains("sutura_rate_limit_buckets{tier=\"general\"} 7"));
        assert!(!rendered.contains("something_else"));
    }

    #[test]
    fn a_histogram_tracks_buckets_sum_and_count_with_fractional_sum() {
        let mut builder = RegistryBuilder::default();
        let histogram = builder.histogram("sutura_question_duration_seconds", &[0.1, 1.0, 10.0]);
        histogram.observe(0.05);
        histogram.observe(5.0);
        histogram.observe(20.0);
        let rendered = builder.build().render();
        assert!(rendered.contains("sutura_question_duration_seconds_bucket{le=\"0.1\"} 1"));
        assert!(rendered.contains("sutura_question_duration_seconds_bucket{le=\"1\"} 1"));
        assert!(rendered.contains("sutura_question_duration_seconds_bucket{le=\"10\"} 2"));
        assert!(rendered.contains("sutura_question_duration_seconds_bucket{le=\"+Inf\"} 3"));
        // 0.05 + 5.0 + 20.0 = 25.05 - the fractional part must survive the sum, not round to 25.
        assert!(rendered.contains("sutura_question_duration_seconds_sum 25.05"));
        assert!(rendered.contains("sutura_question_duration_seconds_count 3"));
    }

    #[test]
    fn an_empty_registry_renders_nothing() {
        assert_eq!(Registry::new().render(), "");
    }

    #[test]
    fn registry_is_immutable_after_build() {
        // The type-level claim: after `build`, the only door is `render`. There is deliberately no
        // `&self` registration to compile here, so this test exercises that the built value renders
        // the series that were registered, and that a second render agrees.
        let mut builder = RegistryBuilder::default();
        let gauge = builder.gauge("sutura_execution_slots");
        gauge.set(4);
        let registry = builder.build();
        assert_eq!(registry.render(), registry.render());
        assert!(registry.render().contains("sutura_execution_slots 4"));
    }

    #[test]
    fn a_label_is_a_static_str_and_renders_its_value() {
        assert_eq!(label("verified").as_str(), "verified");
        assert_eq!(label("deployment").as_str(), "deployment");
    }

    #[test]
    fn handles_share_the_registry_atomics() {
        let mut builder = RegistryBuilder::default();
        let counter = builder.counter("sutura_questions_total");
        let registry = builder.build();
        counter.inc();
        // The handle and the render read the same underlying atomic.
        assert!(registry.render().contains("sutura_questions_total 1"));
    }
}
