use crate::{GrmError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValueArg {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryTerm {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCommand {
    Help,
    Exit,
    ModelDefine { args: Vec<String> },
    ModelList,
    ModelShow { name: String },
    LinkDefine { args: Vec<String> },
    LinkList,
    LinkShow { name: String },
    NodeCreate { model_name: String, assignments: Vec<KeyValueArg> },
    NodeFind { model_name: String, terms: Vec<QueryTerm> },
    NodeUpdate { model_name: String, id: String, assignments: Vec<KeyValueArg> },
    NodeDelete { model_name: String, id: String },
    EdgeCreate { model_name: String, assignments: Vec<KeyValueArg> },
    EdgeFind { model_name: String, terms: Vec<QueryTerm> },
    EdgeUpdate { model_name: String, id: String, assignments: Vec<KeyValueArg> },
    EdgeDelete { model_name: String, id: String },
    SessionSave { args: Vec<String> },
    SessionLoad { args: Vec<String> },
    SessionAutocommit { args: Vec<String> },
    Unknown { raw: String },
}

pub fn parse_command_line(input: &str) -> Result<SessionCommand> {
    let tokens = tokenize_command_line(input.trim())?;
    if tokens.is_empty() {
        return Ok(SessionCommand::Unknown {
            raw: String::new(),
        });
    }

    let command = tokens[0].as_str();
    let args = &tokens[1..];

    match command {
        "?" | "help" | "session.help" => Ok(SessionCommand::Help),
        "exit" | "session.exit" => Ok(SessionCommand::Exit),
        "model.define" => Ok(SessionCommand::ModelDefine {
            args: args.to_vec(),
        }),
        "model.list" => Ok(SessionCommand::ModelList),
        "model.show" => Ok(SessionCommand::ModelShow {
            name: expect_single_arg(command, args)?.to_string(),
        }),
        "link.define" => Ok(SessionCommand::LinkDefine {
            args: args.to_vec(),
        }),
        "link.list" => Ok(SessionCommand::LinkList),
        "link.show" => Ok(SessionCommand::LinkShow {
            name: expect_single_arg(command, args)?.to_string(),
        }),
        "node.create" => Ok(SessionCommand::NodeCreate {
            model_name: required_positional(command, args, 0)?.to_string(),
            assignments: parse_assignments(&args[1..])?,
        }),
        "node.find" => Ok(SessionCommand::NodeFind {
            model_name: required_positional(command, args, 0)?.to_string(),
            terms: parse_query_terms(&args[1..])?,
        }),
        "node.update" | "node.edit" => Ok(SessionCommand::NodeUpdate {
            model_name: required_positional(command, args, 0)?.to_string(),
            id: required_positional(command, args, 1)?.to_string(),
            assignments: parse_assignments(&args[2..])?,
        }),
        "node.delete" => Ok(SessionCommand::NodeDelete {
            model_name: required_positional(command, args, 0)?.to_string(),
            id: required_positional(command, args, 1)?.to_string(),
        }),
        "edge.create" => Ok(SessionCommand::EdgeCreate {
            model_name: required_positional(command, args, 0)?.to_string(),
            assignments: parse_assignments(&args[1..])?,
        }),
        "edge.find" => Ok(SessionCommand::EdgeFind {
            model_name: required_positional(command, args, 0)?.to_string(),
            terms: parse_query_terms(&args[1..])?,
        }),
        "edge.update" | "edge.edit" => Ok(SessionCommand::EdgeUpdate {
            model_name: required_positional(command, args, 0)?.to_string(),
            id: required_positional(command, args, 1)?.to_string(),
            assignments: parse_assignments(&args[2..])?,
        }),
        "edge.delete" => Ok(SessionCommand::EdgeDelete {
            model_name: required_positional(command, args, 0)?.to_string(),
            id: required_positional(command, args, 1)?.to_string(),
        }),
        "session.save" => Ok(SessionCommand::SessionSave {
            args: args.to_vec(),
        }),
        "session.load" => Ok(SessionCommand::SessionLoad {
            args: args.to_vec(),
        }),
        "session.autocommit" => Ok(SessionCommand::SessionAutocommit {
            args: args.to_vec(),
        }),
        _ => Ok(SessionCommand::Unknown {
            raw: input.trim().to_string(),
        }),
    }
}

pub fn tokenize_command_line(input: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        match quote {
            Some(q) => match ch {
                '\\' => {
                    let Some(next) = chars.next() else {
                        return Err(GrmError::Constraint(
                            "unterminated escape sequence in quoted string".into(),
                        ));
                    };
                    current.push(match next {
                        'n' => '\n',
                        't' => '\t',
                        '\\' => '\\',
                        '\'' => '\'',
                        '"' => '"',
                        other => {
                            return Err(GrmError::Constraint(format!(
                                "invalid escape sequence '\\{}' in quoted string",
                                other
                            )))
                        }
                    });
                }
                _ if ch == q => quote = None,
                _ => current.push(ch),
            },
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                }
                _ => current.push(ch),
            },
        }
    }

    if quote.is_some() {
        return Err(GrmError::Constraint("unterminated quoted string".into()));
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    Ok(tokens)
}

pub fn parse_query_terms_from_strs(args: &[&str]) -> Result<Vec<QueryTerm>> {
    let mut terms = Vec::new();
    for arg in args {
        let (key, value) = split_query_term(arg)?;
        terms.push(QueryTerm {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(terms)
}

fn parse_assignments(args: &[String]) -> Result<Vec<KeyValueArg>> {
    let mut assignments = Vec::new();
    for arg in args {
        let Some((key, value)) = arg.split_once('=') else {
            return Err(GrmError::Constraint(format!(
                "expected key=value argument, got '{}'",
                arg
            )));
        };
        assignments.push(KeyValueArg {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(assignments)
}

fn parse_query_terms(args: &[String]) -> Result<Vec<QueryTerm>> {
    parse_query_terms_from_strs(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

fn split_query_term(arg: &str) -> Result<(&str, &str)> {
    if arg.contains(">>") || arg.contains("<<") {
        return Err(GrmError::Constraint(format!(
            "invalid query term '{}'",
            arg
        )));
    }

    for operator in ["!=", ">=", "<=", ">", "<", "~", "="] {
        if let Some((key, value)) = arg.split_once(operator) {
            if key.is_empty() || value.is_empty() {
                return Err(GrmError::Constraint(format!(
                    "invalid query term '{}'",
                    arg
                )));
            }
            let result = (
                match operator {
                    "=" => key,
                    "!=" => &arg[..key.len() + 1],
                    ">=" | "<=" => &arg[..key.len() + 2],
                    ">" | "<" | "~" => &arg[..key.len() + 1],
                    _ => key,
                },
                value,
            );
            if result.0 == "order" {
                validate_order_term_shape(result.1)?;
            }
            return Ok(result);
        }
    }

    Err(GrmError::Constraint(format!(
        "invalid query term '{}'",
        arg
    )))
}

fn validate_order_term_shape(raw: &str) -> Result<()> {
    for segment in raw.split(',') {
        let Some((field, direction)) = segment.split_once(':') else {
            return Err(GrmError::Constraint(
                "order must use order=<field>:asc|desc[,<field>:asc|desc ...]".into(),
            ));
        };

        if field.is_empty() {
            return Err(GrmError::Constraint(
                "order must use order=<field>:asc|desc[,<field>:asc|desc ...]".into(),
            ));
        }

        match direction {
            "asc" | "desc" => {}
            _ => {
                return Err(GrmError::Constraint(
                    "order direction must be asc or desc".into(),
                ))
            }
        }
    }

    Ok(())
}

fn expect_single_arg<'a>(command: &str, args: &'a [String]) -> Result<&'a str> {
    if args.len() != 1 {
        return Err(GrmError::Constraint(format!("usage: {command} <name>")));
    }
    Ok(args[0].as_str())
}

fn required_positional<'a>(command: &str, args: &'a [String], index: usize) -> Result<&'a str> {
    args.get(index).map(String::as_str).ok_or_else(|| {
        GrmError::Constraint(format!("missing required argument for {}", command))
    })
}
