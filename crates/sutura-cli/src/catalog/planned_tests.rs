use sutura_domain::model::SourceName;

#[test]
fn planned_only_files_require_the_sql_feature_at_boot() {
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_domain::warehouse::Warehouse as _;
    let source = SourceName::parse("local").expect("a source");
    let posture = SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(
            AcknowledgementReason::parse("the deployment reads its own fixture files").expect("an acknowledgement"),
        ),
    };
    let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
        source.clone(),
        posture,
        sutura_exec_datafusion::WorkingSet::of_bytes(core::num::NonZeroUsize::new(1 << 20).expect("positive")),
    )
    .expect("a bounded file engine");
    assert!(!engine.accepts_raw_statements());
    let engines = sutura_app::Warehouses::of(engine);
    assert_eq!(super::raw_capable(&engines, &[source]).is_ok(), cfg!(feature = "sql"));
    assert!(super::raw_capable(&engines, &[SourceName::parse("missing").expect("a source")]).is_err());
}
