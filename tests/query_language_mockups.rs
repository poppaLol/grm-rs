#[test]
fn query_language_mockups_capture_comparison_and_quoted_value_examples() {
    let examples = [
        r#"node.find User name!="Alice Jones""#,
        "node.find User age>40",
        "node.find User age>=21",
        r#"node.find User bio~"graph databases""#,
    ];

    for example in examples {
        assert!(example.starts_with("node.find User "));
    }
}

#[test]
fn query_language_mockups_capture_ordering_and_paging_examples() {
    let examples = [
        "node.find User age>=21 order=age:desc limit=10",
        "node.find User active=true order=name:asc offset=20 limit=10",
        "edge.find Authored year>=2020 order=year:desc limit=5",
        "node.find User active=true order=age:desc,name:asc limit=10",
        "edge.find Authored from=1 order=year:desc,to:asc",
    ];

    assert!(examples.iter().any(|line| line.contains("order=")));
    assert!(examples.iter().any(|line| line.contains("limit=")));
    assert!(examples.iter().any(|line| line.contains("offset=")));
    assert!(examples.iter().any(|line| line.contains("order=age:desc,name:asc")));
}

#[test]
fn query_language_mockups_capture_edge_endpoint_examples() {
    let examples = [
        "edge.find Authored from=1",
        "edge.find Authored to=2 year>=2024",
        "edge.find Authored from=1 year>=2024 order=year:desc limit=10",
    ];

    assert!(examples.iter().all(|line| line.starts_with("edge.find Authored ")));
}

#[test]
fn query_language_mockups_capture_expected_output_shapes() {
    let node_output = [
        "2 nodes matched model 'User'.",
        r#"Node User userId=2 {name="Bob", age=43, active=true}"#,
    ];
    let edge_output = [
        "1 edge matched link 'Authored'.",
        "Edge Authored authoredId=3 from=1 to=2 {year=2024}",
    ];

    assert!(node_output[0].contains("matched model"));
    assert!(edge_output[0].contains("matched link"));
}
