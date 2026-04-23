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
fn query_language_mockups_capture_output_format_examples() {
    let examples = [
        "node.find User age>=21",
        "node.find User age>=21 format=default",
        "node.find User age>=21 format=jsonl",
        "node.find User age>=21 order=age:desc format=table",
        "edge.find Authored from=1 format=jsonl",
        "traverse.find User start=1 depth<=2 format=graph",
    ];

    assert!(examples.iter().any(|line| !line.contains("format=")));
    assert!(examples.iter().any(|line| line.contains("format=default")));
    assert!(examples.iter().any(|line| line.contains("format=jsonl")));
    assert!(examples.iter().any(|line| line.contains("format=table")));
    assert!(examples.iter().any(|line| line.contains("format=graph")));
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
    let jsonl_node_output = [
        r#"{"kind":"node","model":"User","id":2,"labels":["User"],"props":{"name":"Bob","age":43,"active":true}}"#,
        r#"{"kind":"node","model":"User","id":5,"labels":["User"],"props":{"name":"Carol","age":41,"active":false}}"#,
    ];
    let jsonl_edge_output = [
        r#"{"kind":"edge","model":"Authored","id":3,"from":1,"to":2,"type":"Authored","props":{"year":2024}}"#,
    ];
    let table_output = [
        "+--------+-------------+-----+--------+",
        "| userId | name        | age | active |",
        "| 2      | Bob         | 43  | true   |",
    ];
    let graph_output = [
        r#"(User#1 {name="Alice"})"#,
        r#"  +--[Authored#3 {year=2024}]--> (Post#2 {title="Hello"})"#,
    ];

    assert!(node_output[0].contains("matched model"));
    assert!(edge_output[0].contains("matched link"));
    assert!(jsonl_node_output.iter().all(|line| line.starts_with("{\"kind\":\"node\"")));
    assert!(jsonl_edge_output.iter().all(|line| line.starts_with("{\"kind\":\"edge\"")));
    assert!(table_output.iter().any(|line| line.contains("| userId |")));
    assert!(graph_output.iter().any(|line| line.contains("[Authored#3")));
}
