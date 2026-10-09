//! `when` clauses on selection actions (P1, P10), e.g. `selection.words <= 3`.
//!
//! A small, closed language: known variables compared with numbers or strings,
//! joined with `&&`, `||`, `!` and parentheses. The core checks the syntax at
//! install; the reader evaluates it (src/lib/extensions/when.ts, the same grammar).

pub const VARIABLES: &[&str] = &[
    "selection.words",
    "selection.chars",
    "selection.language",
    "book.language",
    "book.fixedLayout",
    // Reading Lens LK9.
    "book.lang",
    "selection.sentences",
];

#[derive(Debug, PartialEq)]
enum Token {
    Ident(String),
    Num,
    Str,
    Op,
    And,
    Or,
    Not,
    Open,
    Close,
}

fn tokens(s: &str) -> Result<Vec<Token>, String> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = vec![];
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' => i += 1,
            b'(' => {
                out.push(Token::Open);
                i += 1
            }
            b')' => {
                out.push(Token::Close);
                i += 1
            }
            b'&' | b'|' => {
                if b.get(i + 1) != Some(&c) {
                    return Err(format!("use {0}{0}", c as char));
                }
                out.push(if c == b'&' { Token::And } else { Token::Or });
                i += 2
            }
            b'<' | b'>' | b'=' | b'!' => {
                let two = b.get(i + 1) == Some(&b'=');
                if c == b'!' && !two {
                    out.push(Token::Not);
                    i += 1;
                } else if c == b'=' && !two {
                    return Err("use == to compare".into());
                } else {
                    out.push(Token::Op);
                    i += if two { 2 } else { 1 }
                }
            }
            b'"' => {
                let end = s[i + 1..].find('"').ok_or("unclosed string")?;
                out.push(Token::Str);
                i += end + 2
            }
            b'0'..=b'9' => {
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1
                }
                out.push(Token::Num)
            }
            b'a'..=b'z' | b'A'..=b'Z' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                    i += 1
                }
                let word = &s[start..i];
                if word == "true" || word == "false" {
                    out.push(Token::Num)
                } else if VARIABLES.contains(&word) {
                    out.push(Token::Ident(word.into()))
                } else {
                    return Err(format!("unknown variable “{word}”"));
                }
            }
            _ => return Err(format!("unexpected “{}”", c as char)),
        }
    }
    Ok(out)
}

/// expr := and ('||' and)* ; and := unary ('&&' unary)* ;
/// unary := '!' unary | '(' expr ')' | term (op term)? ; term := ident | number | string
pub fn parse(s: &str) -> Result<(), String> {
    if s.len() > 200 {
        return Err("too long".into());
    }
    let t = tokens(s)?;
    let mut p = 0;
    expr(&t, &mut p)?;
    if p != t.len() {
        return Err("unexpected text at the end".into());
    }
    Ok(())
}

fn expr(t: &[Token], p: &mut usize) -> Result<(), String> {
    and(t, p)?;
    while t.get(*p) == Some(&Token::Or) {
        *p += 1;
        and(t, p)?;
    }
    Ok(())
}

fn and(t: &[Token], p: &mut usize) -> Result<(), String> {
    unary(t, p)?;
    while t.get(*p) == Some(&Token::And) {
        *p += 1;
        unary(t, p)?;
    }
    Ok(())
}

fn unary(t: &[Token], p: &mut usize) -> Result<(), String> {
    match t.get(*p) {
        Some(Token::Not) => {
            *p += 1;
            unary(t, p)
        }
        Some(Token::Open) => {
            *p += 1;
            expr(t, p)?;
            if t.get(*p) != Some(&Token::Close) {
                return Err("missing )".into());
            }
            *p += 1;
            Ok(())
        }
        _ => {
            term(t, p)?;
            if t.get(*p) == Some(&Token::Op) {
                *p += 1;
                term(t, p)?;
            }
            Ok(())
        }
    }
}

fn term(t: &[Token], p: &mut usize) -> Result<(), String> {
    match t.get(*p) {
        Some(Token::Ident(_) | Token::Num | Token::Str) => {
            *p += 1;
            Ok(())
        }
        _ => Err("expected a variable, number or string".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn accepts_the_closed_language() {
        for ok in [
            "selection.words <= 3",
            "selection.words <= 3 && selection.language == \"en\"",
            "!book.fixedLayout || (selection.chars > 0 && selection.chars < 500)",
            // LK9: Explain's condition.
            "(book.lang == \"en\" || book.lang == \"\") && selection.sentences <= 1",
        ] {
            assert!(parse(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn refuses_anything_else() {
        for bad in [
            "selection.words = 3",
            "window.location",
            "selection.words <=",
            "(selection.words > 1",
            "selection.words > 1 & book.fixedLayout",
            "fetch(\"x\")",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
