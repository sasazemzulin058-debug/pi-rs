//! JSON event contract fixture validation.

#[test]
fn fake_provider_event_fixture_is_json() {
    let fixture: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/json-events.fake.json")).unwrap();
    assert_eq!(fixture[0]["type"], "agent_start");
    assert_eq!(fixture[2]["type"], "text_delta");
}
