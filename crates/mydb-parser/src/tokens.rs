//! Small text helpers shared by the parser's rules.

/// Words that carry no meaning for identifying a table.
const NOISE_WORDS: [&str; 18] = [
    "show", "list", "find", "get", "select", "display", "fetch", "read", "delete", "remove",
    "erase", "me", "all", "every", "the", "from", "in", "records",
];

/// Reduces a subject phrase to the words that could name a table.
pub fn strip_noise_words(subject: &str) -> Vec<String> {
    subject
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_'))
        .filter(|word| !word.is_empty() && !NOISE_WORDS.contains(word))
        .map(|word| word.to_lowercase())
        .collect()
}

/// Normalises a phrase for comparison against a column name.
///
/// Lowercases, drops anything that is not a letter or digit, and strips a
/// trailing "ed" or "s" from each word. This is what lets "signed up" reach
/// the `signup_date` column: ["signed", "up"] stems to ["sign", "up"], which
/// joins to "signup".
pub fn normalise_phrase(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(stem)
        .collect::<Vec<_>>()
        .join("")
}

/// Strips a common English inflection. Deliberately crude: it only needs to
/// make typed phrases and column names meet in the middle.
fn stem(word: &str) -> String {
    let lowered = word.to_lowercase();
    for suffix in ["ed", "s"] {
        if let Some(base) = lowered.strip_suffix(suffix) {
            if base.len() >= 3 {
                return base.to_string();
            }
        }
    }
    lowered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_words_are_removed_leaving_the_table_name() {
        assert_eq!(strip_noise_words("delete every user"), vec!["user"]);
        assert_eq!(strip_noise_words("show me all the users"), vec!["users"]);
    }

    #[test]
    fn phrases_normalise_to_match_column_names() {
        assert_eq!(normalise_phrase("signup date"), "signupdate");
        assert_eq!(normalise_phrase("signed up"), "signup");
        assert_eq!(normalise_phrase("full_name"), "full_name".replace('_', ""));
    }
}
