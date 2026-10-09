//! Writing identifiers and values into generated SQL, per engine.

use crate::Engine;

/// `ident` quoted: `"name"` on Postgres, `` `name` `` on MySQL.
pub fn quote_ident(engine: Engine, ident: &str) -> String {
    match engine {
        Engine::Postgres => format!("\"{}\"", ident.replace('"', "\"\"")),
        Engine::Mysql => format!("`{}`", ident.replace('`', "``")),
    }
}

/// `ident` as typed by hand: quoted only when it has to be.
pub fn quote_ident_if_needed(engine: Engine, ident: &str) -> String {
    let mut chars = ident.chars();
    let plain = match engine {
        // Unquoted Postgres identifiers fold to lower case.
        Engine::Postgres => {
            chars
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        Engine::Mysql => {
            !ident.is_empty()
                && !ident.chars().all(|c| c.is_ascii_digit())
                && ident
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        }
    };
    if plain {
        ident.to_owned()
    } else {
        quote_ident(engine, ident)
    }
}

/// `text` as a string literal. MySQL also escapes backslashes, which is
/// right unless the server runs with `NO_BACKSLASH_ESCAPES`.
pub fn quote_literal(engine: Engine, text: &str) -> String {
    match engine {
        Engine::Postgres => format!("'{}'", text.replace('\'', "''")),
        Engine::Mysql => format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers() {
        assert_eq!(quote_ident(Engine::Postgres, "a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_ident(Engine::Mysql, "a`b"), "`a``b`");
        assert_eq!(quote_ident_if_needed(Engine::Postgres, "orders"), "orders");
        assert_eq!(
            quote_ident_if_needed(Engine::Postgres, "Orders"),
            "\"Orders\""
        );
        assert_eq!(quote_ident_if_needed(Engine::Mysql, "Orders"), "Orders");
        assert_eq!(quote_ident_if_needed(Engine::Mysql, "123"), "`123`");
    }

    #[test]
    fn literals() {
        assert_eq!(quote_literal(Engine::Postgres, "it's \\"), "'it''s \\'");
        assert_eq!(quote_literal(Engine::Mysql, "it's \\"), "'it''s \\\\'");
    }
}
