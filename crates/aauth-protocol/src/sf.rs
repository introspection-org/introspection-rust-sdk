//! The slice of RFC 8941 Structured Field Values the signature headers use:
//! dictionaries whose members are items or inner lists, with parameters.

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Bare {
    Integer(i64),
    Decimal(String),
    String(String),
    Token(String),
    Bytes(Vec<u8>),
    Boolean(bool),
}

pub(crate) type Params = Vec<(String, Bare)>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub bare: Bare,
    pub params: Params,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Member {
    Item(Item),
    InnerList(Vec<Item>, Params),
}

impl Bare {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Bare::String(s) | Bare::Token(s) => Some(s),
            _ => None,
        }
    }
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

type Result<T> = std::result::Result<T, String>;

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn skip_sp(&mut self) {
        while self.peek() == Some(b' ') {
            self.pos += 1;
        }
    }

    fn skip_ows(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    fn key(&mut self) -> Result<String> {
        let start = self.pos;
        match self.peek() {
            Some(c) if c.is_ascii_lowercase() || c == b'*' => self.pos += 1,
            _ => return Err("expected a key".into()),
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-.*".contains(&c))
        {
            self.pos += 1;
        }
        Ok(String::from_utf8_lossy(&self.input[start..self.pos]).into_owned())
    }

    fn params(&mut self) -> Result<Params> {
        let mut params = Vec::new();
        while self.peek() == Some(b';') {
            self.pos += 1;
            self.skip_sp();
            let key = self.key()?;
            let value = if self.peek() == Some(b'=') {
                self.pos += 1;
                self.bare()?
            } else {
                Bare::Boolean(true)
            };
            if let Some(existing) = params.iter_mut().find(|(k, _)| *k == key) {
                existing.1 = value;
            } else {
                params.push((key, value));
            }
        }
        Ok(params)
    }

    fn bare(&mut self) -> Result<Bare> {
        match self.peek() {
            Some(b'"') => self.string(),
            Some(b':') => self.bytes(),
            Some(b'?') => {
                self.pos += 1;
                match self.bump() {
                    Some(b'1') => Ok(Bare::Boolean(true)),
                    Some(b'0') => Ok(Bare::Boolean(false)),
                    _ => Err("bad boolean".into()),
                }
            }
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == b'*' => self.token(),
            _ => Err("expected a bare item".into()),
        }
    }

    fn string(&mut self) -> Result<Bare> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.bump() {
                Some(b'\\') => match self.bump() {
                    Some(c @ (b'"' | b'\\')) => out.push(c as char),
                    _ => return Err("bad escape in string".into()),
                },
                Some(b'"') => return Ok(Bare::String(out)),
                Some(c) if (0x20..0x7f).contains(&c) => out.push(c as char),
                _ => return Err("unterminated string".into()),
            }
        }
    }

    fn token(&mut self) -> Result<Bare> {
        let start = self.pos;
        self.pos += 1;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~:/".contains(&c))
        {
            self.pos += 1;
        }
        Ok(Bare::Token(
            String::from_utf8_lossy(&self.input[start..self.pos]).into_owned(),
        ))
    }

    fn bytes(&mut self) -> Result<Bare> {
        use base64::Engine;
        self.pos += 1;
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || b"+/=".contains(&c)) {
            self.pos += 1;
        }
        let encoded = &self.input[start..self.pos];
        if self.bump() != Some(b':') {
            return Err("unterminated byte sequence".into());
        }
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map(Bare::Bytes)
            .map_err(|e| format!("bad byte sequence: {e}"))
    }

    fn number(&mut self) -> Result<Bare> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let mut decimal = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if c == b'.' && !decimal {
                decimal = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.input[start..self.pos]).map_err(|e| e.to_string())?;
        if decimal {
            return Ok(Bare::Decimal(text.to_string()));
        }
        if text.trim_start_matches('-').len() > 15 {
            return Err("integer too long".into());
        }
        text.parse()
            .map(Bare::Integer)
            .map_err(|_| "bad integer".into())
    }

    fn item(&mut self) -> Result<Item> {
        let bare = self.bare()?;
        let params = self.params()?;
        Ok(Item { bare, params })
    }

    fn member(&mut self) -> Result<Member> {
        if self.peek() != Some(b'(') {
            return self.item().map(Member::Item);
        }
        self.pos += 1;
        let mut items = Vec::new();
        loop {
            self.skip_sp();
            if self.peek() == Some(b')') {
                self.pos += 1;
                let params = self.params()?;
                return Ok(Member::InnerList(items, params));
            }
            items.push(self.item()?);
            match self.peek() {
                Some(b' ' | b')') => {}
                _ => return Err("bad inner list".into()),
            }
        }
    }
}

/// Parse a Dictionary into its members in order.
pub(crate) fn parse_dictionary(input: &str) -> Result<Vec<(String, Member)>> {
    let mut p = Parser {
        input: input.as_bytes(),
        pos: 0,
    };
    let mut out: Vec<(String, Member)> = Vec::new();
    p.skip_sp();
    while p.peek().is_some() {
        let key = p.key()?;
        let member = if p.peek() == Some(b'=') {
            p.pos += 1;
            p.member()?
        } else {
            Member::Item(Item {
                bare: Bare::Boolean(true),
                params: p.params()?,
            })
        };
        if let Some(existing) = out.iter_mut().find(|(k, _)| *k == key) {
            existing.1 = member;
        } else {
            out.push((key, member));
        }
        p.skip_ows();
        if p.peek().is_none() {
            break;
        }
        if p.bump() != Some(b',') {
            return Err("expected a comma between members".into());
        }
        p.skip_ows();
        if p.peek().is_none() {
            return Err("trailing comma".into());
        }
    }
    Ok(out)
}

fn serialize_bare(bare: &Bare, out: &mut String) {
    use base64::Engine;
    match bare {
        Bare::Integer(i) => out.push_str(&i.to_string()),
        Bare::Decimal(d) => out.push_str(d),
        Bare::String(s) => {
            out.push('"');
            for c in s.chars() {
                if c == '"' || c == '\\' {
                    out.push('\\');
                }
                out.push(c);
            }
            out.push('"');
        }
        Bare::Token(t) => out.push_str(t),
        Bare::Bytes(b) => {
            out.push(':');
            out.push_str(&base64::engine::general_purpose::STANDARD.encode(b));
            out.push(':');
        }
        Bare::Boolean(b) => out.push_str(if *b { "?1" } else { "?0" }),
    }
}

fn serialize_params(params: &Params, out: &mut String) {
    for (key, value) in params {
        out.push(';');
        out.push_str(key);
        if *value != Bare::Boolean(true) {
            out.push('=');
            serialize_bare(value, out);
        }
    }
}

/// An inner list, serialized as RFC 8941 §4.1.1.1 prescribes.
pub(crate) fn serialize_inner_list(items: &[Item], params: &Params) -> String {
    let mut out = String::from("(");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        serialize_bare(&item.bare, &mut out);
        serialize_params(&item.params, &mut out);
    }
    out.push(')');
    serialize_params(params, &mut out);
    out
}

/// A String, quoted and escaped.
pub(crate) fn quote(value: &str) -> String {
    let mut out = String::new();
    serialize_bare(&Bare::String(value.to_string()), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_input_round_trips() {
        let raw = r#"sig=("@method" "@authority" "@path" "signature-key");created=1791000100"#;
        let dict = parse_dictionary(raw).unwrap();
        let Member::InnerList(items, params) = &dict[0].1 else {
            panic!()
        };
        assert_eq!(items.len(), 4);
        assert_eq!(format!("sig={}", serialize_inner_list(items, params)), raw);
    }

    #[test]
    fn signature_key_parses_scheme_and_params() {
        let dict = parse_dictionary(r#"sig=jwt;jwt="a.b.c""#).unwrap();
        let Member::Item(item) = &dict[0].1 else {
            panic!()
        };
        assert_eq!(item.bare, Bare::Token("jwt".into()));
        assert_eq!(item.params[0].1, Bare::String("a.b.c".into()));
    }

    #[test]
    fn malformed_input_is_refused() {
        for bad in ["sig=(", "sig=\"abc", "sig=:!!:", "a=1,", "A=1"] {
            assert!(parse_dictionary(bad).is_err(), "{bad}");
        }
    }
}
