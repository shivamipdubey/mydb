//! A description of what a connected database contains.
//!
//! The parser needs this to resolve "users" in a command to a real table, and
//! the preview screen needs it to render what a write will affect. It lives in
//! the shared crate so neither has to depend on an adapter.

use serde::{Deserialize, Serialize};

/// A column on a table or collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// The engine's own name for the type, shown to the user as-is rather than
    /// mapped to some invented cross-engine vocabulary.
    pub data_type: String,
    pub nullable: bool,
}

/// A table (or, for document engines in phase 2, a collection).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    /// The namespace the table lives in: a Postgres schema, a MySQL database.
    pub namespace: String,
    pub name: String,
    pub columns: Vec<Column>,
}

impl Table {
    /// The name a user would type, qualified only when it needs to be.
    pub fn display_name(&self) -> String {
        if self.namespace == "public" {
            self.name.clone()
        } else {
            format!("{}.{}", self.namespace, self.name)
        }
    }

    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }
}

/// Everything MYDB knows about the structure of one connection's database.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub tables: Vec<Table>,
}

impl Schema {
    /// Finds a table by the name a user typed.
    ///
    /// Matching is case-insensitive on the bare name, because a user typing
    /// "Users" means the `users` table. An ambiguous match across namespaces
    /// returns `None` rather than guessing: picking one silently could target
    /// the wrong table, which is exactly the class of mistake the preview step
    /// exists to prevent.
    pub fn find_table(&self, name: &str) -> Option<&Table> {
        let wanted = name.trim().to_lowercase();

        if let Some((namespace, bare)) = wanted.split_once('.') {
            return self.tables.iter().find(|t| {
                t.namespace.to_lowercase() == namespace && t.name.to_lowercase() == bare
            });
        }

        let mut matches = self
            .tables
            .iter()
            .filter(|t| t.name.to_lowercase() == wanted);
        let first = matches.next()?;
        match matches.next() {
            None => Some(first),
            Some(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(namespace: &str, name: &str) -> Table {
        Table {
            namespace: namespace.to_string(),
            name: name.to_string(),
            columns: vec![Column {
                name: "id".to_string(),
                data_type: "integer".to_string(),
                nullable: false,
            }],
        }
    }

    #[test]
    fn finds_a_table_case_insensitively() {
        let schema = Schema {
            tables: vec![table("public", "users")],
        };
        assert!(schema.find_table("Users").is_some());
        assert!(schema.find_table("  users ").is_some());
    }

    #[test]
    fn finds_a_namespace_qualified_table() {
        let schema = Schema {
            tables: vec![table("analytics", "users")],
        };
        assert!(schema.find_table("analytics.users").is_some());
        assert!(schema.find_table("users").is_some());
    }

    #[test]
    fn an_ambiguous_name_resolves_to_nothing_rather_than_a_guess() {
        let schema = Schema {
            tables: vec![table("public", "users"), table("analytics", "users")],
        };
        assert!(
            schema.find_table("users").is_none(),
            "two tables share this name; guessing one could target the wrong table"
        );
        assert!(schema.find_table("public.users").is_some());
    }

    #[test]
    fn display_name_qualifies_only_outside_the_default_namespace() {
        assert_eq!(table("public", "users").display_name(), "users");
        assert_eq!(
            table("analytics", "users").display_name(),
            "analytics.users"
        );
    }
}
