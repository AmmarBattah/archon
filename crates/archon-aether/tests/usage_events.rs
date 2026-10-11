//! Usage fixtures follow Aether's EventKind::AgentUsageUpdated wire shape.
use archon_aether::events::{EventKind, parse_event};
use archon_core::cost::CostBasis;

#[test]
fn real_usage_events_preserve_the_cost_basis() {
    for (fixture, basis) in [
        (
            include_str!("fixtures/usage_list_price.json"),
            CostBasis::ListPrice,
        ),
        (
            include_str!("fixtures/usage_provider_reported.json"),
            CostBasis::ProviderReported,
        ),
    ] {
        let event = parse_event(serde_json::from_str(fixture).unwrap()).unwrap();
        let EventKind::AgentUsageUpdated { usage } = event.kind else {
            panic!("usage event expected")
        };
        assert_eq!(usage.cost_basis, basis);
        assert_eq!(usage.cost_usd, 0.5);
        assert_eq!(usage.cost_complete, Some(true));
        assert_eq!(usage.input_tokens, 1200);
        assert_eq!(usage.estimated_cost_usd, None);
    }
}

#[test]
fn missing_basis_is_unknown_and_invalid_basis_is_visible() {
    let mut wire: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/usage_list_price.json")).unwrap();
    wire["kind"]["total"]
        .as_object_mut()
        .unwrap()
        .remove("cost_basis");
    let EventKind::AgentUsageUpdated { usage } = parse_event(wire.clone()).unwrap().kind else {
        panic!("usage event expected")
    };
    assert_eq!(usage.cost_basis, CostBasis::Unknown);
    wire["kind"]["total"]["cost_basis"] = serde_json::json!("billed");
    assert!(parse_event(wire).is_err());
}
