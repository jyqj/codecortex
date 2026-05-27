use super::ast::Token;
use cc_model::{CcError, CcResult};

// ── Lexer ──────────────────────────────────────────

pub fn tokenize(input: &str) -> CcResult<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut pos = 0;

    while pos < len {
        // Skip whitespace.
        if chars[pos].is_whitespace() {
            pos += 1;
            continue;
        }

        // Multi-char operators and arrows first.
        if pos + 1 < len {
            let two = &input[pos..pos + 2];
            match two {
                "->" => {
                    tokens.push(Token::Arrow);
                    pos += 2;
                    continue;
                }
                "<-" => {
                    // Disambiguate: this is a Dash then we handle direction in parser.
                    // Actually we treat <- as two tokens: Lt + Dash -- but for Cypher
                    // it's easier to handle in parser. Let's push as special handling.
                    // We'll let the parser deal with direction via Dash/Arrow/Lt tokens.
                    // Actually, let's just be pragmatic: push separate tokens.
                    tokens.push(Token::Lt);
                    tokens.push(Token::Dash);
                    pos += 2;
                    continue;
                }
                "=~" => {
                    tokens.push(Token::RegexMatch);
                    pos += 2;
                    continue;
                }
                "<>" => {
                    tokens.push(Token::Neq);
                    pos += 2;
                    continue;
                }
                "!=" => {
                    tokens.push(Token::Neq);
                    pos += 2;
                    continue;
                }
                "<=" => {
                    tokens.push(Token::Lte);
                    pos += 2;
                    continue;
                }
                ">=" => {
                    tokens.push(Token::Gte);
                    pos += 2;
                    continue;
                }
                ".." => {
                    tokens.push(Token::DotDot);
                    pos += 2;
                    continue;
                }
                _ => {}
            }
        }

        // Single-char symbols.
        match chars[pos] {
            '(' => {
                tokens.push(Token::LParen);
                pos += 1;
                continue;
            }
            ')' => {
                tokens.push(Token::RParen);
                pos += 1;
                continue;
            }
            '[' => {
                tokens.push(Token::LBracket);
                pos += 1;
                continue;
            }
            ']' => {
                tokens.push(Token::RBracket);
                pos += 1;
                continue;
            }
            '{' => {
                tokens.push(Token::LBrace);
                pos += 1;
                continue;
            }
            '}' => {
                tokens.push(Token::RBrace);
                pos += 1;
                continue;
            }
            ':' => {
                tokens.push(Token::Colon);
                pos += 1;
                continue;
            }
            '.' => {
                tokens.push(Token::Dot);
                pos += 1;
                continue;
            }
            ',' => {
                tokens.push(Token::Comma);
                pos += 1;
                continue;
            }
            '-' => {
                tokens.push(Token::Dash);
                pos += 1;
                continue;
            }
            '*' => {
                tokens.push(Token::Star);
                pos += 1;
                continue;
            }
            '=' => {
                tokens.push(Token::Eq);
                pos += 1;
                continue;
            }
            '<' => {
                tokens.push(Token::Lt);
                pos += 1;
                continue;
            }
            '>' => {
                tokens.push(Token::Gt);
                pos += 1;
                continue;
            }
            _ => {}
        }

        // String literals (single or double quoted).
        if chars[pos] == '\'' || chars[pos] == '"' {
            let quote = chars[pos];
            pos += 1;
            let mut s = String::new();
            while pos < len && chars[pos] != quote {
                if chars[pos] == '\\' && pos + 1 < len {
                    pos += 1; // skip backslash, take the next char literally
                    match chars[pos] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        other => s.push(other), // \', \", \\, etc.
                    }
                } else {
                    s.push(chars[pos]);
                }
                pos += 1;
            }
            if pos < len {
                pos += 1; // skip closing quote
            }
            tokens.push(Token::StringLit(s));
            continue;
        }

        // Numbers.
        if chars[pos].is_ascii_digit() {
            let start = pos;
            while pos < len && chars[pos].is_ascii_digit() {
                pos += 1;
            }
            if pos < len && chars[pos] == '.' && pos + 1 < len && chars[pos + 1].is_ascii_digit() {
                pos += 1;
                while pos < len && chars[pos].is_ascii_digit() {
                    pos += 1;
                }
                let s: String = chars[start..pos].iter().collect();
                let f = s
                    .parse::<f64>()
                    .map_err(|_| CcError::Search(format!("invalid float: {s}")))?;
                tokens.push(Token::FloatLit(f));
            } else {
                let s: String = chars[start..pos].iter().collect();
                let n = s
                    .parse::<i64>()
                    .map_err(|_| CcError::Search(format!("invalid integer: {s}")))?;
                tokens.push(Token::IntLit(n));
            }
            continue;
        }

        // Identifiers and keywords.
        if chars[pos].is_alphabetic() || chars[pos] == '_' {
            let start = pos;
            while pos < len && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
                pos += 1;
            }
            let word: String = chars[start..pos].iter().collect();
            let upper = word.to_uppercase();

            // Check for two-word keywords.
            let token = match upper.as_str() {
                "MATCH" => Token::Match,
                "WHERE" => Token::Where,
                "RETURN" => Token::Return,
                "LIMIT" => Token::Limit,
                "AND" => Token::And,
                "OR" => Token::Or,
                "NOT" => Token::Not,
                "AS" => Token::As,
                "ASC" => Token::Asc,
                "DESC" => Token::Desc,
                "CONTAINS" => Token::Contains,
                "COUNT" => Token::Count,
                "SUM" => Token::Sum,
                "AVG" => Token::Avg,
                "MIN" => Token::Min,
                "MAX" => Token::Max,
                "COLLECT" => Token::Collect,
                "DISTINCT" => Token::Distinct,
                "OPTIONAL" => Token::Optional,
                "UNION" => {
                    // Look ahead for ALL.
                    let saved = pos;
                    while pos < len && chars[pos].is_whitespace() {
                        pos += 1;
                    }
                    let all_start = pos;
                    while pos < len && chars[pos].is_alphabetic() {
                        pos += 1;
                    }
                    let next_word: String = chars[all_start..pos].iter().collect();
                    if next_word.to_uppercase() == "ALL" {
                        Token::UnionAll
                    } else {
                        // Not UNION ALL, backtrack.
                        pos = saved;
                        Token::Union
                    }
                }
                "ORDER" => {
                    // Look ahead for BY.
                    let saved = pos;
                    while pos < len && chars[pos].is_whitespace() {
                        pos += 1;
                    }
                    let by_start = pos;
                    while pos < len && chars[pos].is_alphabetic() {
                        pos += 1;
                    }
                    let next_word: String = chars[by_start..pos].iter().collect();
                    if next_word.to_uppercase() == "BY" {
                        Token::OrderBy
                    } else {
                        // Not ORDER BY, backtrack.
                        pos = saved;
                        Token::Ident(word)
                    }
                }
                "STARTS" => {
                    // Look ahead for WITH.
                    let saved = pos;
                    while pos < len && chars[pos].is_whitespace() {
                        pos += 1;
                    }
                    let w_start = pos;
                    while pos < len && chars[pos].is_alphabetic() {
                        pos += 1;
                    }
                    let next_word: String = chars[w_start..pos].iter().collect();
                    if next_word.to_uppercase() == "WITH" {
                        Token::StartsWith
                    } else {
                        pos = saved;
                        Token::Ident(word)
                    }
                }
                "ENDS" => {
                    // Look ahead for WITH.
                    let saved = pos;
                    while pos < len && chars[pos].is_whitespace() {
                        pos += 1;
                    }
                    let w_start = pos;
                    while pos < len && chars[pos].is_alphabetic() {
                        pos += 1;
                    }
                    let next_word: String = chars[w_start..pos].iter().collect();
                    if next_word.to_uppercase() == "WITH" {
                        Token::EndsWith
                    } else {
                        pos = saved;
                        Token::Ident(word)
                    }
                }
                "TRUE" | "FALSE" | "NULL" => Token::Ident(word),
                _ => Token::Ident(word),
            };
            tokens.push(token);
            continue;
        }

        // Skip unknown characters.
        pos += 1;
    }

    tokens.push(Token::Eof);
    Ok(tokens)
}
