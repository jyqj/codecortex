//! Case-sensitive Python-style file patterns: '/' is ordinary, braces literal.
//! Matching is bounded dynamic programming, not a backtracking regular expression.
use super::{invalid, Result};
#[derive(Debug)]
enum Token {
    Star,
    Any,
    Literal(char),
    Class {
        negative: bool,
        ranges: Vec<(char, char)>,
    },
}
fn tokens(pattern: &str) -> Vec<Token> {
    let p: Vec<char> = pattern.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            '*' => {
                if !matches!(out.last(), Some(Token::Star)) {
                    out.push(Token::Star);
                }
                i += 1;
            }
            '?' => {
                out.push(Token::Any);
                i += 1;
            }
            '[' => {
                let start = i + 1;
                let mut end = start;
                if end < p.len() && p[end] == '!' {
                    end += 1;
                }
                if end < p.len() && p[end] == ']' {
                    end += 1;
                }
                while end < p.len() && p[end] != ']' {
                    end += 1;
                }
                if end == p.len() {
                    out.push(Token::Literal('['));
                    i += 1;
                    continue;
                }
                let negative = p.get(start) == Some(&'!');
                let mut j = start + usize::from(negative);
                let mut ranges = Vec::new();
                while j < end {
                    if j + 2 < end && p[j + 1] == '-' {
                        if p[j] <= p[j + 2] {
                            ranges.push((p[j], p[j + 2]));
                        }
                        j += 3;
                    } else {
                        ranges.push((p[j], p[j]));
                        j += 1;
                    }
                }
                out.push(Token::Class { negative, ranges });
                i = end + 1;
            }
            c => {
                out.push(Token::Literal(c));
                i += 1;
            }
        }
    }
    out
}
pub fn matches(actual: &str, pattern: &str) -> Result<bool> {
    if actual.len() > 4096 || pattern.len() > 4096 {
        return Err(invalid("path pattern length limit"));
    }
    let chars: Vec<char> = actual.chars().collect();
    let mut previous = vec![false; chars.len() + 1];
    previous[0] = true;
    for token in tokens(pattern) {
        let mut current = vec![false; chars.len() + 1];
        if matches!(token, Token::Star) {
            current[0] = previous[0];
        }
        for (j, c) in chars.iter().enumerate() {
            current[j + 1] = match &token {
                Token::Star => previous[j + 1] || current[j],
                Token::Any => previous[j],
                Token::Literal(l) => previous[j] && c == l,
                Token::Class { negative, ranges } => {
                    previous[j] && (ranges.iter().any(|(a, b)| a <= c && c <= b) != *negative)
                }
            };
        }
        previous = current;
    }
    Ok(previous[chars.len()])
}
