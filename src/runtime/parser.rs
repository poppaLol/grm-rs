use crate::{GrmError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedToken {
    text: String,
    start: usize,
}

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
    SessionDescribe,
    ModelDefine {
        args: Vec<String>,
    },
    ModelList,
    ModelShow {
        name: String,
    },
    LinkDefine {
        args: Vec<String>,
    },
    LinkList,
    LinkShow {
        name: String,
    },
    NodeCreate {
        model_name: String,
        assignments: Vec<KeyValueArg>,
    },
    NodeFind {
        model_name: String,
        terms: Vec<QueryTerm>,
    },
    NodeUpdate {
        model_name: String,
        id: String,
        assignments: Vec<KeyValueArg>,
    },
    NodeDelete {
        model_name: String,
        id: String,
    },
    EdgeCreate {
        model_name: String,
        assignments: Vec<KeyValueArg>,
    },
    EdgeFind {
        model_name: String,
        terms: Vec<QueryTerm>,
    },
    EdgeUpdate {
        model_name: String,
        id: String,
        assignments: Vec<KeyValueArg>,
    },
    EdgeDelete {
        model_name: String,
        id: String,
    },
    SessionSave {
        args: Vec<String>,
    },
    SessionLoad {
        args: Vec<String>,
    },
    SessionImport {
        args: Vec<String>,
    },
    SessionExport {
        args: Vec<String>,
    },
    SessionAutocommit {
        args: Vec<String>,
    },
    Unknown {
        raw: String,
    },
}

pub fn parse_command_line(input: &str) -> Result<SessionCommand> {
    let trimmed = input.trim();
    let tokens = tokenize_command_line_internal(trimmed)?;
    if tokens.is_empty() {
        return Ok(SessionCommand::Unknown { raw: String::new() });
    }

    let command = tokens[0].text.as_str();
    let args = &tokens[1..];

    match command {
        "?" | "help" | "session.help" => Ok(SessionCommand::Help),
        "exit" | "session.exit" => Ok(SessionCommand::Exit),
        "session.describe" => Ok(SessionCommand::SessionDescribe),
        "model.define" => Ok(SessionCommand::ModelDefine {
            args: args.iter().map(|token| token.text.clone()).collect(),
        }),
        "model.list" => Ok(SessionCommand::ModelList),
        "model.show" => Ok(SessionCommand::ModelShow {
            name: expect_single_arg(command, args)?.to_string(),
        }),
        "link.define" => Ok(SessionCommand::LinkDefine {
            args: args.iter().map(|token| token.text.clone()).collect(),
        }),
        "link.list" => Ok(SessionCommand::LinkList),
        "link.show" => Ok(SessionCommand::LinkShow {
            name: expect_single_arg(command, args)?.to_string(),
        }),
        "node.create" => Ok(SessionCommand::NodeCreate {
            model_name: required_positional(command, args, 0)?.to_string(),
            assignments: parse_assignments(&args[1..], trimmed)?,
        }),
        "node.find" => Ok(SessionCommand::NodeFind {
            model_name: required_positional(command, args, 0)?.to_string(),
            terms: parse_query_terms(&args[1..], trimmed)?,
        }),
        "node.update" | "node.edit" => Ok(SessionCommand::NodeUpdate {
            model_name: required_positional(command, args, 0)?.to_string(),
            id: required_positional(command, args, 1)?.to_string(),
            assignments: parse_assignments(&args[2..], trimmed)?,
        }),
        "node.delete" => Ok(SessionCommand::NodeDelete {
            model_name: required_positional(command, args, 0)?.to_string(),
            id: required_positional(command, args, 1)?.to_string(),
        }),
        "edge.create" => Ok(SessionCommand::EdgeCreate {
            model_name: required_positional(command, args, 0)?.to_string(),
            assignments: parse_assignments(&args[1..], trimmed)?,
        }),
        "edge.find" => Ok(SessionCommand::EdgeFind {
            model_name: required_positional(command, args, 0)?.to_string(),
            terms: parse_query_terms(&args[1..], trimmed)?,
        }),
        "edge.update" | "edge.edit" => Ok(SessionCommand::EdgeUpdate {
            model_name: required_positional(command, args, 0)?.to_string(),
            id: required_positional(command, args, 1)?.to_string(),
            assignments: parse_assignments(&args[2..], trimmed)?,
        }),
        "edge.delete" => Ok(SessionCommand::EdgeDelete {
            model_name: required_positional(command, args, 0)?.to_string(),
            id: required_positional(command, args, 1)?.to_string(),
        }),
        "session.save" => Ok(SessionCommand::SessionSave {
            args: args.iter().map(|token| token.text.clone()).collect(),
        }),
        "session.load" => Ok(SessionCommand::SessionLoad {
            args: args.iter().map(|token| token.text.clone()).collect(),
        }),
        "session.import" => Ok(SessionCommand::SessionImport {
            args: args.iter().map(|token| token.text.clone()).collect(),
        }),
        "session.export" => Ok(SessionCommand::SessionExport {
            args: args.iter().map(|token| token.text.clone()).collect(),
        }),
        "session.autocommit" => Ok(SessionCommand::SessionAutocommit {
            args: args.iter().map(|token| token.text.clone()).collect(),
        }),
        _ => Ok(SessionCommand::Unknown {
            raw: trimmed.to_string(),
        }),
    }
}

fn tokenize_command_line_internal(input: &str) -> Result<Vec<ParsedToken>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_start = 0usize;
    let mut chars = input.char_indices().peekable();
    let mut quote: Option<char> = None;
    let mut quote_start: Option<usize> = None;

    while let Some((index, ch)) = chars.next() {
        match quote {
            Some(q) => match ch {
                '\\' => {
                    let Some((_, next)) = chars.next() else {
                        return Err(constraint_at(
                            input,
                            index,
                            "unterminated escape sequence in quoted string",
                        ));
                    };
                    current.push(match next {
                        'n' => '\n',
                        't' => '\t',
                        '\\' => '\\',
                        '\'' => '\'',
                        '"' => '"',
                        other => {
                            return Err(constraint_at(
                                input,
                                index,
                                format!("invalid escape sequence '\\{}' in quoted string", other),
                            ));
                        }
                    });
                }
                _ if ch == q => {
                    quote = None;
                    quote_start = None;
                }
                _ => current.push(ch),
            },
            None => match ch {
                '"' | '\'' => {
                    if current.is_empty() {
                        current_start = index;
                    }
                    quote = Some(ch);
                    quote_start = Some(index);
                }
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        tokens.push(ParsedToken {
                            text: std::mem::take(&mut current),
                            start: current_start,
                        });
                    }
                }
                _ => {
                    if current.is_empty() {
                        current_start = index;
                    }
                    current.push(ch)
                }
            },
        }
    }

    if quote.is_some() {
        return Err(constraint_at(
            input,
            quote_start.unwrap_or(input.len().saturating_sub(1)),
            "unterminated quoted string",
        ));
    }

    if !current.is_empty() {
        tokens.push(ParsedToken {
            text: current,
            start: current_start,
        });
    }

    Ok(tokens)
}

pub fn parse_query_terms_from_strs(args: &[&str]) -> Result<Vec<QueryTerm>> {
    let mut terms = Vec::new();
    for arg in args {
        let (key, value) = split_query_term(arg, arg, 0)?;
        terms.push(QueryTerm {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(terms)
}

fn parse_assignments(args: &[ParsedToken], input: &str) -> Result<Vec<KeyValueArg>> {
    let mut assignments = Vec::new();
    for arg in args {
        let Some((key, value)) = arg.text.split_once('=') else {
            return Err(constraint_at(
                input,
                arg.start,
                format!("expected key=value argument, got '{}'", arg.text),
            ));
        };
        assignments.push(KeyValueArg {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(assignments)
}

fn parse_query_terms(args: &[ParsedToken], input: &str) -> Result<Vec<QueryTerm>> {
    let mut terms = Vec::new();
    for arg in args {
        let (key, value) = split_query_term(&arg.text, input, arg.start)?;
        terms.push(QueryTerm {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(terms)
}

fn split_query_term<'a>(arg: &'a str, input: &str, start: usize) -> Result<(&'a str, &'a str)> {
    if arg.contains(">>") || arg.contains("<<") {
        return Err(constraint_at(
            input,
            start,
            format!("invalid query term '{}'", arg),
        ));
    }

    for operator in ["!=", ">=", "<=", ">", "<", "~", "="] {
        if let Some((key, value)) = arg.split_once(operator) {
            if key.is_empty() || value.is_empty() {
                return Err(constraint_at(
                    input,
                    start,
                    format!("invalid query term '{}'", arg),
                ));
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
                validate_order_term_shape(result.1, input, start)?;
            }
            if result.0 == "format" {
                validate_format_term_shape(result.1, input, start)?;
            }
            return Ok(result);
        }
    }

    Err(constraint_at(
        input,
        start,
        format!("invalid query term '{}'", arg),
    ))
}

fn validate_format_term_shape(raw: &str, input: &str, start: usize) -> Result<()> {
    match raw {
        "default" | "jsonl" | "table" | "graph" => Ok(()),
        _ => Err(constraint_at(
            input,
            start,
            "format must be one of: default, jsonl, table, graph",
        )),
    }
}

fn validate_order_term_shape(raw: &str, input: &str, start: usize) -> Result<()> {
    for segment in raw.split(',') {
        let Some((field, direction)) = segment.split_once(':') else {
            return Err(constraint_at(
                input,
                start,
                "order must use order=<field>:asc|desc[,<field>:asc|desc ...]",
            ));
        };

        if field.is_empty() {
            return Err(constraint_at(
                input,
                start,
                "order must use order=<field>:asc|desc[,<field>:asc|desc ...]",
            ));
        }

        match direction {
            "asc" | "desc" => {}
            _ => {
                return Err(constraint_at(
                    input,
                    start,
                    "order direction must be asc or desc",
                ));
            }
        }
    }

    Ok(())
}

fn expect_single_arg<'a>(command: &str, args: &'a [ParsedToken]) -> Result<&'a str> {
    if args.len() != 1 {
        return Err(GrmError::Constraint(format!("usage: {command} <name>")));
    }
    Ok(args[0].text.as_str())
}

fn required_positional<'a>(
    command: &str,
    args: &'a [ParsedToken],
    index: usize,
) -> Result<&'a str> {
    args.get(index)
        .map(|token| token.text.as_str())
        .ok_or_else(|| GrmError::Constraint(format!("missing required argument for {}", command)))
}

fn constraint_at(input: &str, offset: usize, message: impl Into<String>) -> GrmError {
    let offset = offset.min(input.len());
    let line_start = input[..offset].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let line_end = input[offset..]
        .find('\n')
        .map(|idx| offset + idx)
        .unwrap_or(input.len());
    let line = &input[line_start..line_end];
    let line_number = input[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let column = input[line_start..offset].chars().count() + 1;
    let caret_pad = " ".repeat(column.saturating_sub(1));

    GrmError::Constraint(format!(
        "{} at line {}, column {}\n{}\n{}^",
        message.into(),
        line_number,
        column,
        line,
        caret_pad
    ))
}
