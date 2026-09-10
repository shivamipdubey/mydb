//! Turns typed (later spoken) text into a structured `Intent`.
//!
//! docs/10-nlp-voice-and-local-model.md: phase 1's parser is rule-based, not a
//! model. Confidence tiers and the ambiguity fallback ladder arrive in phase 5.
//!
//! The rule this parser is built around: it never guesses. If it cannot
//! resolve the operation, the table, a column, or a value, it returns an error
//! naming what it could not resolve. A plausible-looking wrong intent is the
//! most dangerous thing this module could produce, and while the preview step
//! exists to catch exactly that (docs/23-risk-register.md), a parser that
//! declines to guess means the user sees a clear question instead of a
//! confidently wrong query.
//!
//! Phase 1 (T6) parses reads and deletes. Anything else is rejected as not yet
//! supported rather than approximated.

mod error;
mod tokens;

pub use error::ParseError;

use mydb_core::{Comparison, Condition, Engine, Filter, Intent, Operation, Schema, Table, Value};

use tokens::{normalise_phrase, strip_noise_words};

/// Words that begin a read.
const READ_VERBS: [&str; 8] = [
    "show", "list", "find", "get", "select", "display", "fetch", "read",
];

/// Words that begin a delete.
const DELETE_VERBS: [&str; 3] = ["delete", "remove", "erase"];

/// Operations phase 1 does not parse yet. Recognised explicitly so the user is
/// told the operation is not supported, rather than having their command fail
/// as unintelligible or, worse, be matched to something else.
const NOT_YET_SUPPORTED: [(&str, &str); 6] = [
    ("insert", "INSERT"),
    ("add", "INSERT"),
    ("create", "INSERT"),
    ("update", "UPDATE"),
    ("set", "UPDATE"),
    ("truncate", "TRUNCATE"),
];

/// Phrases that introduce a filter.
const FILTER_INTRODUCERS: [&str; 6] = [
    " where ", " whose ", " who ", " that ", " with ", " having ",
];

/// Parses a command against a known schema.
///
/// The schema is required, not optional: a table or column name is only
/// meaningful once it has been resolved against the real database, and
/// resolving it here means nothing downstream ever handles a name the user
/// merely typed.
pub fn parse(input: &str, schema: &Schema, engine: Engine) -> Result<Intent, ParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ParseError::Empty);
    }

    // to_ascii_lowercase, not to_lowercase: it maps only ASCII letters and so
    // is guaranteed to preserve byte length. That lets every offset found in
    // the lowercased text index the original safely, which is what keeps a
    // user's value from being case-folded on its way into the filter.
    let lowered = trimmed.to_ascii_lowercase();

    let (subject_end, filter_start) = split_filter(&lowered);
    let subject = &lowered[..subject_end];

    // The operation is detected from the subject alone, never the filter. A
    // value like "my table" or an injection attempt containing DROP TABLE
    // must not change what operation the command is understood to be.
    let operation = detect_operation(subject)?;
    let table = resolve_table(subject, schema)?;

    let filter = match filter_start {
        Some(start) => parse_filter(&lowered[start..], &trimmed[start..], table)?,
        None => Filter::everything(),
    };

    Ok(Intent {
        engine,
        namespace: table.namespace.clone(),
        table: table.name.clone(),
        operation,
        filter,
    })
}

/// Works out what the command asks for from its leading verb.
fn detect_operation(subject: &str) -> Result<Operation, ParseError> {
    let first = subject.split_whitespace().next().unwrap_or_default();

    if DELETE_VERBS.contains(&first) {
        // "delete table x" and "drop" are schema operations, not row deletes.
        // They arrive in T11 with their own preview (current schema and row
        // count, not matching rows), so treating them as a row delete here
        // would show the user the wrong preview entirely.
        //
        // This looks at the subject only. Scanning the whole command would let
        // a filter value containing the word "table" change the operation.
        if subject.split_whitespace().any(|word| word == "table") {
            return Err(ParseError::NotYetSupported {
                operation: "DROP TABLE".to_string(),
            });
        }
        return Ok(Operation::Delete);
    }

    if READ_VERBS.contains(&first) || subject.starts_with("how many") {
        return Ok(Operation::Read);
    }

    if first == "drop" {
        return Err(ParseError::NotYetSupported {
            operation: "DROP TABLE".to_string(),
        });
    }

    for (verb, operation) in NOT_YET_SUPPORTED {
        if first == verb {
            return Err(ParseError::NotYetSupported {
                operation: operation.to_string(),
            });
        }
    }

    Err(ParseError::UnknownOperation {
        input: subject.trim().to_string(),
    })
}

/// Locates the boundary between a command's subject and its filter clause.
///
/// Returns byte offsets rather than slices so the caller can index both the
/// lowercased text and the original with the same positions.
fn split_filter(lowered: &str) -> (usize, Option<usize>) {
    // The earliest introducer wins, so "delete users where x" splits at
    // "where" even though later words might also introduce a clause.
    let earliest = FILTER_INTRODUCERS
        .iter()
        .filter_map(|introducer| lowered.find(introducer).map(|at| (at, introducer.len())))
        .min_by_key(|(at, _)| *at);

    match earliest {
        Some((at, length)) => (at, Some(at + length)),
        None => (lowered.len(), None),
    }
}

/// Finds which table the command targets.
fn resolve_table<'a>(subject: &str, schema: &'a Schema) -> Result<&'a Table, ParseError> {
    let candidates = strip_noise_words(subject);
    if candidates.is_empty() {
        return Err(ParseError::NoTarget);
    }

    // Try each remaining word, and its singular form, against the schema.
    // "delete every user" should reach the `users` table.
    for candidate in &candidates {
        if let Some(table) = schema.find_table(candidate) {
            return Ok(table);
        }
        if let Some(table) = schema.find_table(&pluralise(candidate)) {
            return Ok(table);
        }
    }

    // Distinguish "I have never heard of this table" from "this name matches
    // more than one table", because the user's next action differs.
    for candidate in &candidates {
        let plural = pluralise(candidate);
        for name in [candidate.as_str(), plural.as_str()] {
            let matches = schema
                .tables
                .iter()
                .filter(|t| t.name.to_lowercase() == name)
                .count();
            if matches > 1 {
                return Err(ParseError::AmbiguousTable {
                    name: name.to_string(),
                });
            }
        }
    }

    Err(ParseError::UnknownTable {
        name: candidates.join(" "),
        known: schema.tables.iter().map(Table::display_name).collect(),
    })
}

/// Naive English pluralisation, enough to match a typed singular against a
/// conventionally plural table name.
fn pluralise(word: &str) -> String {
    if word.ends_with('s') {
        word.to_string()
    } else if word.ends_with('y') && word.len() > 1 {
        format!("{}ies", &word[..word.len() - 1])
    } else {
        format!("{word}s")
    }
}

/// Parses the clause after "where", "whose", and friends.
///
/// Takes the lowercased text and the original together, indexed identically,
/// so keywords can be matched case-insensitively while values keep the case
/// the user typed.
fn parse_filter(lowered: &str, original: &str, table: &Table) -> Result<Filter, ParseError> {
    const SEPARATOR: &str = " and ";

    let mut conditions = Vec::new();
    let mut start = 0;
    loop {
        let end = lowered[start..]
            .find(SEPARATOR)
            .map(|offset| start + offset)
            .unwrap_or(lowered.len());

        conditions.push(parse_condition(
            &lowered[start..end],
            &original[start..end],
            table,
        )?);

        if end == lowered.len() {
            break;
        }
        start = end + SEPARATOR.len();
    }

    Ok(Filter { conditions })
}

/// Comparison phrases, longest first so "is not" is tried before "is".
const COMPARISONS: [(&str, Comparison); 20] = [
    (
        "is greater than or equal to",
        Comparison::GreaterThanOrEqual,
    ),
    ("is less than or equal to", Comparison::LessThanOrEqual),
    ("is not equal to", Comparison::NotEquals),
    ("is greater than", Comparison::GreaterThan),
    ("is less than", Comparison::LessThan),
    ("is on or before", Comparison::LessThanOrEqual),
    ("is on or after", Comparison::GreaterThanOrEqual),
    ("greater than", Comparison::GreaterThan),
    ("less than", Comparison::LessThan),
    ("on or before", Comparison::LessThanOrEqual),
    ("on or after", Comparison::GreaterThanOrEqual),
    ("is equal to", Comparison::Equals),
    ("is before", Comparison::LessThan),
    ("is after", Comparison::GreaterThan),
    ("at least", Comparison::GreaterThanOrEqual),
    ("at most", Comparison::LessThanOrEqual),
    ("is not", Comparison::NotEquals),
    ("before", Comparison::LessThan),
    ("after", Comparison::GreaterThan),
    ("is", Comparison::Equals),
];

fn parse_condition(lowered: &str, original: &str, table: &Table) -> Result<Condition, ParseError> {
    for (phrase, comparison) in COMPARISONS {
        let padded = format!(" {phrase} ");
        if let Some(position) = lowered.find(&padded) {
            let value_at = position + padded.len();

            let column = resolve_column(lowered[..position].trim(), table)?;
            let value = parse_value(
                original[value_at..].trim(),
                lowered[value_at..].trim(),
                &column.data_type,
                &column.name,
            )?;

            return Ok(Condition {
                column: column.name.clone(),
                comparison,
                value,
            });
        }
    }

    Err(ParseError::UnparseableCondition {
        clause: original.trim().to_string(),
    })
}

/// Matches a typed phrase to a real column.
///
/// Handles "signup date" for `signup_date`, and "signed up" for the same
/// column, by comparing normalised, lightly stemmed forms and by also trying
/// the column name with a trailing `_date`, `_at`, or `_time` removed.
fn resolve_column<'a>(text: &str, table: &'a Table) -> Result<&'a mydb_core::Column, ParseError> {
    let wanted = normalise_phrase(text);
    if wanted.is_empty() {
        return Err(ParseError::UnparseableCondition {
            clause: text.to_string(),
        });
    }

    for column in &table.columns {
        let name = normalise_phrase(&column.name);
        if name == wanted {
            return Ok(column);
        }
        for suffix in ["date", "at", "time"] {
            if let Some(base) = name.strip_suffix(suffix) {
                if !base.is_empty() && base == wanted {
                    return Ok(column);
                }
            }
        }
    }

    Err(ParseError::UnknownColumn {
        column: text.to_string(),
        table: table.display_name(),
        known: table.columns.iter().map(|c| c.name.clone()).collect(),
    })
}

/// Converts the text after a comparison into a typed value.
///
/// The column's own type decides the interpretation, so `active is false`
/// becomes a boolean and `id is 3` an integer, rather than everything becoming
/// text and relying on the database to coerce it.
fn parse_value(
    text: &str,
    lowered: &str,
    data_type: &str,
    column: &str,
) -> Result<Value, ParseError> {
    // The value keeps the case the user typed; only keyword recognition below
    // uses the lowercased form. An email or a name is data, not syntax.
    let cleaned = text.trim().trim_matches(['"', '\'']).trim();
    let cleaned_lower = lowered.trim().trim_matches(['"', '\'']).trim();

    if cleaned.is_empty() {
        return Err(ParseError::UnparseableValue {
            value: text.to_string(),
            column: column.to_string(),
            expected: data_type.to_string(),
        });
    }

    if cleaned_lower == "null" || cleaned_lower == "nothing" {
        return Ok(Value::Null);
    }

    let unparseable = || ParseError::UnparseableValue {
        value: text.to_string(),
        column: column.to_string(),
        expected: data_type.to_string(),
    };

    match data_type {
        "boolean" => match cleaned_lower {
            "true" | "yes" | "active" => Ok(Value::Boolean(true)),
            "false" | "no" | "inactive" => Ok(Value::Boolean(false)),
            _ => Err(unparseable()),
        },
        "integer" | "bigint" | "smallint" => cleaned
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|_| unparseable()),
        "numeric" | "real" | "double precision" => cleaned
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| unparseable()),
        "date" | "timestamp without time zone" | "timestamp with time zone" => {
            parse_date(cleaned).map(Value::Date).ok_or_else(unparseable)
        }
        _ => Ok(Value::Text(cleaned.to_string())),
    }
}

/// Accepts a full ISO date, or a bare year meaning that year's first day.
///
/// "before 2024" is a natural way to say "before 2024-01-01", and refusing it
/// would make the parser feel broken for a phrasing users will reach for.
fn parse_date(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split('-').collect();
    match parts.as_slice() {
        [year] if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) => {
            Some(format!("{year}-01-01"))
        }
        [year, month, day]
            if year.len() == 4
                && month.len() == 2
                && day.len() == 2
                && text.chars().all(|c| c.is_ascii_digit() || c == '-') =>
        {
            let month_number: u8 = month.parse().ok()?;
            let day_number: u8 = day.parse().ok()?;
            if (1..=12).contains(&month_number) && (1..=31).contains(&day_number) {
                Some(text.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}
