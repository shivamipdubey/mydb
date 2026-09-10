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

use mydb_core::{
    Assignment, Comparison, Condition, Engine, Filter, Intent, Operation, Schema, Table, Value,
};

use tokens::{normalise_phrase, strip_noise_words};

/// Words that begin a read.
const READ_VERBS: [&str; 8] = [
    "show", "list", "find", "get", "select", "display", "fetch", "read",
];

/// Words that begin a delete.
const DELETE_VERBS: [&str; 3] = ["delete", "remove", "erase"];

/// Words that begin an insert.
const INSERT_VERBS: [&str; 3] = ["insert", "add", "create"];

/// Words that begin an update.
const UPDATE_VERBS: [&str; 3] = ["update", "set", "change"];

/// Introduces an insert's values: "add a user with email is ada@example.com".
const INSERT_INTRODUCER: &str = " with ";

/// Introduces an update's values: "update users set active to false".
const UPDATE_INTRODUCER: &str = " set ";

/// Separates an update's target from its values in the "set X for Y" form.
const UPDATE_TARGET_INTRODUCERS: [&str; 3] = [" for ", " on ", " in "];

/// Operations phase 1 does not parse yet. Recognised explicitly so the user is
/// told the operation is not supported, rather than having their command fail
/// as unintelligible or, worse, be matched to something else.
const NOT_YET_SUPPORTED: [(&str, &str); 1] = [("truncate", "TRUNCATE")];

/// Phrases that introduce a filter.
const FILTER_INTRODUCERS: [&str; 6] = [
    " where ", " whose ", " who ", " that ", " with ", " having ",
];

/// Filter introducers for an insert.
///
/// "with" is missing on purpose: an insert uses it to introduce the values it
/// writes, so treating it as a filter would split "add a user with email is
/// ada@example.com" in exactly the wrong place. This is why the operation has
/// to be known before the command is split, not after.
const INSERT_FILTER_INTRODUCERS: [&str; 5] = [" where ", " whose ", " who ", " that ", " having "];

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
    // user's value from being case-folded on its way into the intent.
    let lowered = trimmed.to_ascii_lowercase();

    // The operation comes from the leading verb, before anything is split,
    // because which words introduce a filter depends on which operation this
    // is. It is read from the verb alone and never from the filter, so a
    // value like "my table" cannot change what the command is understood
    // to be.
    let operation = operation_from_verb(&lowered)?;

    let introducers: &[&str] = match operation {
        Operation::Insert => &INSERT_FILTER_INTRODUCERS,
        _ => &FILTER_INTRODUCERS,
    };
    let (subject_end, filter_start) = split_filter(&lowered, introducers);
    let subject = &lowered[..subject_end];

    // "delete table x" is a schema operation, not a row delete. It arrives in
    // T11 with its own preview (current schema and row count, not matching
    // rows), so reading it as a row delete would show the wrong preview
    // entirely. Checked against the subject, so a value containing the word
    // cannot trigger it.
    if operation == Operation::Delete && subject.split_whitespace().any(|word| word == "table") {
        return Err(ParseError::NotYetSupported {
            operation: "DROP TABLE".to_string(),
        });
    }

    // Where the target table is named, and where the written values are,
    // differ by operation. Splitting here keeps each shape's rules in one
    // place instead of spreading special cases through the resolvers.
    let (target_text, assignment_range) = match operation {
        Operation::Read | Operation::Delete => (subject, None),
        Operation::Insert => split_insert(subject)?,
        Operation::Update => split_update(subject)?,
    };

    let table = resolve_table(target_text, schema)?;

    let assignments = match assignment_range {
        Some(range) => parse_assignments(&lowered[range.clone()], &trimmed[range], table)?,
        None => Vec::new(),
    };

    let filter = match filter_start {
        Some(start) => parse_filter(&lowered[start..], &trimmed[start..], table)?,
        None => Filter::everything(),
    };

    if operation == Operation::Insert && !filter.matches_everything() {
        return Err(ParseError::FilterOnInsert);
    }

    Ok(Intent {
        engine,
        namespace: table.namespace.clone(),
        table: table.name.clone(),
        operation,
        filter,
        assignments,
    })
}

/// Splits "add a user with email is ada@example.com" into its target and its
/// values.
fn split_insert(subject: &str) -> Result<(&str, Option<std::ops::Range<usize>>), ParseError> {
    match subject.find(INSERT_INTRODUCER) {
        Some(at) => Ok((
            &subject[..at],
            Some(at + INSERT_INTRODUCER.len()..subject.len()),
        )),
        // An insert with no values would create an empty row, which is almost
        // never what someone meant to type.
        None => Err(ParseError::MissingValues {
            operation: "insert".to_string(),
            hint: "add a user with email is ada@example.com".to_string(),
        }),
    }
}

/// Splits either "update users set active to false" or "set active to false
/// for users" into target and values.
fn split_update(subject: &str) -> Result<(&str, Option<std::ops::Range<usize>>), ParseError> {
    if let Some(at) = subject.find(UPDATE_INTRODUCER) {
        let values = at + UPDATE_INTRODUCER.len()..subject.len();
        return Ok((&subject[..at], Some(values)));
    }

    // The "set X for Y" form: the command opens with the verb, so the values
    // run from after it to whichever target word introduces the table.
    let first_word_end = subject.find(char::is_whitespace).unwrap_or(subject.len());
    for introducer in UPDATE_TARGET_INTRODUCERS {
        if let Some(at) = subject.find(introducer) {
            if at > first_word_end {
                return Ok((&subject[at + introducer.len()..], Some(first_word_end..at)));
            }
        }
    }

    Err(ParseError::MissingValues {
        operation: "update".to_string(),
        hint: "update users set active to false where id is 3".to_string(),
    })
}

/// Phrases that assign a value to a column.
///
/// Longest first, so "is not" cannot be read as "is". Assignment has no
/// negative form, so "is not" is absent deliberately: it would mean nothing
/// here and should fail rather than be silently read as equality.
const ASSIGNMENT_OPERATORS: [&str; 4] = [" to ", " is ", " = ", " as "];

/// Parses "active to false and email to a@b.com" into typed assignments.
fn parse_assignments(
    lowered: &str,
    original: &str,
    table: &Table,
) -> Result<Vec<Assignment>, ParseError> {
    let mut assignments = Vec::new();

    for (start, end) in split_clauses(lowered) {
        let clause_lower = &lowered[start..end];
        let clause_original = &original[start..end];
        if clause_lower.trim().is_empty() {
            continue;
        }
        assignments.push(parse_assignment(clause_lower, clause_original, table)?);
    }

    if assignments.is_empty() {
        return Err(ParseError::MissingValues {
            operation: "write".to_string(),
            hint: "name at least one column and the value to give it".to_string(),
        });
    }

    Ok(assignments)
}

/// Splits a list of clauses on "and" or a comma, returning byte ranges so the
/// caller can index the lowercased text and the original identically.
fn split_clauses(lowered: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut cursor = 0;

    while cursor < lowered.len() {
        let next_and = lowered[cursor..].find(" and ").map(|at| (cursor + at, 5));
        let next_comma = lowered[cursor..].find(',').map(|at| (cursor + at, 1));

        let next = match (next_and, next_comma) {
            (Some(a), Some(c)) => Some(if a.0 <= c.0 { a } else { c }),
            (Some(a), None) => Some(a),
            (None, Some(c)) => Some(c),
            (None, None) => None,
        };

        match next {
            Some((at, width)) => {
                ranges.push((start, at));
                start = at + width;
                cursor = start;
            }
            None => break,
        }
    }
    ranges.push((start, lowered.len()));
    ranges
}

fn parse_assignment(
    lowered: &str,
    original: &str,
    table: &Table,
) -> Result<Assignment, ParseError> {
    for phrase in ASSIGNMENT_OPERATORS {
        if let Some(position) = lowered.find(phrase) {
            let value_at = position + phrase.len();
            let column = resolve_column(lowered[..position].trim(), table)?;
            let value = parse_value(
                original[value_at..].trim(),
                lowered[value_at..].trim(),
                &column.data_type,
                &column.name,
            )?;
            return Ok(Assignment {
                column: column.name.clone(),
                value,
            });
        }
    }

    Err(ParseError::UnparseableAssignment {
        clause: original.trim().to_string(),
    })
}

/// Works out what the command asks for from its leading verb.
fn operation_from_verb(lowered: &str) -> Result<Operation, ParseError> {
    let first = lowered.split_whitespace().next().unwrap_or_default();

    if DELETE_VERBS.contains(&first) {
        return Ok(Operation::Delete);
    }
    if INSERT_VERBS.contains(&first) {
        return Ok(Operation::Insert);
    }
    if UPDATE_VERBS.contains(&first) {
        return Ok(Operation::Update);
    }
    if READ_VERBS.contains(&first) || lowered.starts_with("how many") {
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
        input: lowered.trim().to_string(),
    })
}

/// Locates the boundary between a command's subject and its filter clause.
///
/// Returns byte offsets rather than slices so the caller can index both the
/// lowercased text and the original with the same positions.
fn split_filter(lowered: &str, introducers: &[&str]) -> (usize, Option<usize>) {
    // The earliest introducer wins, so "delete users where x" splits at
    // "where" even though later words might also introduce a clause.
    let earliest = introducers
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
