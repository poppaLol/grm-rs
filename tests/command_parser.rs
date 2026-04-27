use grm_rs::runtime::{KeyValueArg, QueryTerm, SessionCommand, parse_command_line};

#[test]
fn parser_preserves_quoted_values_in_node_find_terms() {
    let command = parse_command_line(
        r#"node.find User name!="Alice Jones" order=age:desc,name:asc limit=10"#,
    )
    .unwrap();

    assert_eq!(
        command,
        SessionCommand::NodeFind {
            model_name: "User".into(),
            terms: vec![
                QueryTerm {
                    key: "name!".into(),
                    value: "Alice Jones".into(),
                },
                QueryTerm {
                    key: "order".into(),
                    value: "age:desc,name:asc".into(),
                },
                QueryTerm {
                    key: "limit".into(),
                    value: "10".into(),
                },
            ],
        }
    );
}

#[test]
fn parser_builds_structured_node_create_assignments() {
    let command = parse_command_line(r#"node.create User name="Alice Jones" age=42"#).unwrap();

    assert_eq!(
        command,
        SessionCommand::NodeCreate {
            model_name: "User".into(),
            assignments: vec![
                KeyValueArg {
                    key: "name".into(),
                    value: "Alice Jones".into(),
                },
                KeyValueArg {
                    key: "age".into(),
                    value: "42".into(),
                },
            ],
        }
    );
}

#[test]
fn parser_builds_structured_edge_find_terms() {
    let command =
        parse_command_line("edge.find Authored from=1 year>=2024 order=year:desc,to:asc").unwrap();

    assert_eq!(
        command,
        SessionCommand::EdgeFind {
            model_name: "Authored".into(),
            terms: vec![
                QueryTerm {
                    key: "from".into(),
                    value: "1".into(),
                },
                QueryTerm {
                    key: "year>=".into(),
                    value: "2024".into(),
                },
                QueryTerm {
                    key: "order".into(),
                    value: "year:desc,to:asc".into(),
                },
            ],
        }
    );
}

#[test]
fn parser_preserves_multi_field_order_term_as_single_query_control() {
    let command = parse_command_line("node.find User active=true order=age:desc,name:asc").unwrap();

    assert_eq!(
        command,
        SessionCommand::NodeFind {
            model_name: "User".into(),
            terms: vec![
                QueryTerm {
                    key: "active".into(),
                    value: "true".into(),
                },
                QueryTerm {
                    key: "order".into(),
                    value: "age:desc,name:asc".into(),
                },
            ],
        }
    );
}

#[test]
fn parser_preserves_output_format_term() {
    let command = parse_command_line("node.find User age>=21 format=jsonl").unwrap();

    assert_eq!(
        command,
        SessionCommand::NodeFind {
            model_name: "User".into(),
            terms: vec![
                QueryTerm {
                    key: "age>=".into(),
                    value: "21".into(),
                },
                QueryTerm {
                    key: "format".into(),
                    value: "jsonl".into(),
                },
            ],
        }
    );
}

#[test]
fn parser_builds_session_export_command() {
    let command = parse_command_line("session.export --json /tmp/grm-export.json").unwrap();

    assert_eq!(
        command,
        SessionCommand::SessionExport {
            args: vec!["--json".into(), "/tmp/grm-export.json".into()],
        }
    );
}

#[test]
fn parser_builds_session_import_command() {
    let command = parse_command_line("session.import --json /tmp/grm-export.json").unwrap();

    assert_eq!(
        command,
        SessionCommand::SessionImport {
            args: vec!["--json".into(), "/tmp/grm-export.json".into()],
        }
    );
}

#[test]
fn parser_preserves_traversal_terms_in_node_find() {
    let command = parse_command_line(
        r#"node.find User name="Alice Jones" via=out:Authored:Post end.title~"Hello" return=edge"#,
    )
    .unwrap();

    assert_eq!(
        command,
        SessionCommand::NodeFind {
            model_name: "User".into(),
            terms: vec![
                QueryTerm {
                    key: "name".into(),
                    value: "Alice Jones".into(),
                },
                QueryTerm {
                    key: "via".into(),
                    value: "out:Authored:Post".into(),
                },
                QueryTerm {
                    key: "end.title~".into(),
                    value: "Hello".into(),
                },
                QueryTerm {
                    key: "return".into(),
                    value: "edge".into(),
                },
            ],
        }
    );
}

#[test]
fn parser_reports_unterminated_quotes() {
    let err = parse_command_line(r#"node.find User name="Alice Jones"#).unwrap_err();
    assert!(err.to_string().contains("unterminated quoted string"));
    assert!(err.to_string().contains("line 1, column"));
    assert!(err.to_string().contains("^"));
}

#[test]
fn parser_reports_invalid_escape_sequences() {
    let err = parse_command_line("node.find User name=\"Alice\\qJones\"").unwrap_err();
    assert!(
        err.to_string()
            .contains("invalid escape sequence '\\q' in quoted string")
    );
    assert!(err.to_string().contains("line 1, column"));
    assert!(err.to_string().contains("^"));
}

#[test]
fn parser_reports_malformed_order_terms() {
    let err = parse_command_line("node.find User order=age").unwrap_err();
    assert!(
        err.to_string()
            .contains("order must use order=<field>:asc|desc[,<field>:asc|desc ...]")
    );
    assert!(err.to_string().contains("^"));

    let err = parse_command_line("node.find User order=age:up").unwrap_err();
    assert!(
        err.to_string()
            .contains("order direction must be asc or desc")
    );
    assert!(err.to_string().contains("^"));
}

#[test]
fn parser_reports_unknown_output_formats() {
    let err = parse_command_line("node.find User format=xml").unwrap_err();
    assert!(
        err.to_string()
            .contains("format must be one of: default, jsonl, table, graph")
    );
    assert!(err.to_string().contains("^"));
}

#[test]
fn parser_reports_invalid_query_term_shapes() {
    let err = parse_command_line("node.find User age>>40").unwrap_err();
    assert!(err.to_string().contains("invalid query term 'age>>40'"));
    assert!(err.to_string().contains("^"));
}

#[test]
fn parser_reports_multiline_error_locations() {
    let err = parse_command_line("node.find User \norder=age:up").unwrap_err();
    assert!(
        err.to_string()
            .contains("order direction must be asc or desc")
    );
    assert!(err.to_string().contains("line 2, column 1"));
    assert!(err.to_string().contains("order=age:up"));
    assert!(err.to_string().contains("^"));
}
