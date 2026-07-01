use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevsetExpr {
    Literal(String),
    Union(Box<RevsetExpr>, Box<RevsetExpr>),
    Intersection(Box<RevsetExpr>, Box<RevsetExpr>),
    Difference(Box<RevsetExpr>, Box<RevsetExpr>),
    Ancestors(Box<RevsetExpr>),
    Descendants(Box<RevsetExpr>),
    Range(Box<RevsetExpr>, Box<RevsetExpr>),
    Function { name: String, args: Vec<RevsetExpr> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    offset: usize,
    message: String,
}

impl ParseError {
    fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

impl std::str::FromStr for RevsetExpr {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse(s)
    }
}

pub fn parse(input: &str) -> Result<RevsetExpr, ParseError> {
    let mut parser = Parser::new(input);
    let expr = parser.parse_expr()?;
    parser.skip_ws();
    if !parser.is_eof() {
        return Err(parser.error("unexpected token"));
    }
    Ok(expr)
}

impl fmt::Display for RevsetExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_with_prec(f, 0, Side::Root)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Side {
    Root,
    Left,
    Right,
    Unary,
}

impl RevsetExpr {
    fn precedence(&self) -> u8 {
        match self {
            RevsetExpr::Union(_, _) => 1,
            RevsetExpr::Intersection(_, _) | RevsetExpr::Difference(_, _) => 2,
            RevsetExpr::Range(_, _) => 3,
            RevsetExpr::Ancestors(_) | RevsetExpr::Descendants(_) => 4,
            RevsetExpr::Literal(_) | RevsetExpr::Function { .. } => 5,
        }
    }

    fn fmt_with_prec(
        &self,
        f: &mut fmt::Formatter<'_>,
        parent_prec: u8,
        side: Side,
    ) -> fmt::Result {
        let prec = self.precedence();
        let needs_parens = prec < parent_prec || (side != Side::Root && prec == parent_prec);
        if needs_parens {
            f.write_str("(")?;
        }

        match self {
            RevsetExpr::Literal(s) => f.write_str(s)?,
            RevsetExpr::Union(lhs, rhs) => {
                lhs.fmt_with_prec(f, prec, Side::Left)?;
                f.write_str(" | ")?;
                rhs.fmt_with_prec(f, prec, Side::Right)?;
            }
            RevsetExpr::Intersection(lhs, rhs) => {
                lhs.fmt_with_prec(f, prec, Side::Left)?;
                f.write_str(" & ")?;
                rhs.fmt_with_prec(f, prec, Side::Right)?;
            }
            RevsetExpr::Difference(lhs, rhs) => {
                lhs.fmt_with_prec(f, prec, Side::Left)?;
                f.write_str(" ~ ")?;
                rhs.fmt_with_prec(f, prec, Side::Right)?;
            }
            RevsetExpr::Ancestors(expr) => {
                f.write_str("::")?;
                expr.fmt_with_prec(f, prec, Side::Unary)?;
            }
            RevsetExpr::Descendants(expr) => {
                expr.fmt_with_prec(f, prec, Side::Unary)?;
                f.write_str("::")?;
            }
            RevsetExpr::Range(lhs, rhs) => {
                lhs.fmt_with_prec(f, prec, Side::Left)?;
                f.write_str("::")?;
                rhs.fmt_with_prec(f, prec, Side::Right)?;
            }
            RevsetExpr::Function { name, args } => {
                f.write_str(name)?;
                f.write_str("(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    arg.fmt_with_prec(f, 0, Side::Root)?;
                }
                f.write_str(")")?;
            }
        }

        if needs_parens {
            f.write_str(")")?;
        }
        Ok(())
    }
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn parse_expr(&mut self) -> Result<RevsetExpr, ParseError> {
        self.parse_union()
    }

    fn parse_union(&mut self) -> Result<RevsetExpr, ParseError> {
        let mut expr = self.parse_intersection_or_difference()?;
        while self.consume_char('|') {
            let rhs = self.parse_intersection_or_difference()?;
            expr = RevsetExpr::Union(Box::new(expr), Box::new(rhs));
        }
        Ok(expr)
    }

    fn parse_intersection_or_difference(&mut self) -> Result<RevsetExpr, ParseError> {
        let mut expr = self.parse_range()?;
        loop {
            if self.consume_char('&') {
                let rhs = self.parse_range()?;
                expr = RevsetExpr::Intersection(Box::new(expr), Box::new(rhs));
            } else if self.consume_char('~') {
                let rhs = self.parse_range()?;
                expr = RevsetExpr::Difference(Box::new(expr), Box::new(rhs));
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_range(&mut self) -> Result<RevsetExpr, ParseError> {
        if self.consume_str("::") {
            let expr = self.parse_range_operand()?;
            return Ok(RevsetExpr::Ancestors(Box::new(expr)));
        }

        let lhs = self.parse_range_operand()?;
        if self.consume_str("::") {
            if self.next_starts_operand() {
                let rhs = self.parse_range_operand()?;
                Ok(RevsetExpr::Range(Box::new(lhs), Box::new(rhs)))
            } else {
                Ok(RevsetExpr::Descendants(Box::new(lhs)))
            }
        } else {
            Ok(lhs)
        }
    }

    fn parse_range_operand(&mut self) -> Result<RevsetExpr, ParseError> {
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<RevsetExpr, ParseError> {
        self.skip_ws();
        if self.consume_char('(') {
            let expr = self.parse_expr()?;
            self.expect_char(')')?;
            return Ok(expr);
        }

        let name = self.parse_ident()?;
        if self.consume_char('(') {
            let mut args = Vec::new();
            self.skip_ws();
            if !self.consume_char(')') {
                loop {
                    args.push(self.parse_expr()?);
                    if self.consume_char(',') {
                        continue;
                    }
                    self.expect_char(')')?;
                    break;
                }
            }
            Ok(RevsetExpr::Function { name, args })
        } else {
            Ok(RevsetExpr::Literal(name))
        }
    }

    fn parse_ident(&mut self) -> Result<String, ParseError> {
        self.skip_ws();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if is_ident_char(ch) {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }

        if self.pos == start {
            Err(self.error("expected revset literal"))
        } else {
            Ok(self.input[start..self.pos].to_owned())
        }
    }

    fn next_starts_operand(&mut self) -> bool {
        self.skip_ws();
        match self.peek_char() {
            Some('(' | ':') => true,
            Some(ch) => is_ident_char(ch),
            None => false,
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        self.skip_ws();
        if self.peek_char() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn consume_str(&mut self, expected: &str) -> bool {
        self.skip_ws();
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            true
        } else {
            false
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<(), ParseError> {
        if self.consume_char(expected) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{expected}'")))
        }
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.pos == self.input.len()
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError::new(self.pos, message)
    }
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '@')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_round_trip(input: &str, expected: &str) {
        let parsed = parse(input).unwrap();
        assert_eq!(parsed.to_string(), expected);
        assert_eq!(parse(&parsed.to_string()).unwrap(), parsed);
    }

    #[test]
    fn parses_literals_and_set_ops_by_precedence() {
        assert_round_trip("a", "a");
        assert_round_trip("a | b & c", "a | b & c");
        assert_round_trip("(a | b) & c", "(a | b) & c");
        assert_round_trip("a ~ b | c", "a ~ b | c");
        assert_round_trip("a & (b ~ c)", "a & (b ~ c)");
    }

    #[test]
    fn parses_ancestor_descendant_and_range_queries() {
        assert_round_trip("a::", "a::");
        assert_round_trip("::a", "::a");
        assert_round_trip("a::b", "a::b");
        assert_round_trip("a::b & ::c", "a::b & ::c");
        assert_round_trip("(a | b)::", "(a | b)::");
    }

    #[test]
    fn parses_functions() {
        assert_round_trip("heads(a::)", "heads(a::)");
        assert_round_trip("heads(a::b | ::c)", "heads(a::b | ::c)");
        assert_round_trip("foo(a, b & c)", "foo(a, b & c)");
    }

    #[test]
    fn rejects_trailing_input() {
        assert!(parse("a b").is_err());
    }

    #[test]
    fn stringifies_constructed_trees_parseably() {
        let a = RevsetExpr::Literal("a".to_owned());
        let b = RevsetExpr::Literal("b".to_owned());
        let c = RevsetExpr::Literal("c".to_owned());

        for expr in [
            RevsetExpr::Ancestors(Box::new(RevsetExpr::Descendants(Box::new(a.clone())))),
            RevsetExpr::Descendants(Box::new(RevsetExpr::Ancestors(Box::new(a.clone())))),
            RevsetExpr::Range(
                Box::new(RevsetExpr::Range(Box::new(a.clone()), Box::new(b.clone()))),
                Box::new(c.clone()),
            ),
            RevsetExpr::Union(
                Box::new(RevsetExpr::Union(Box::new(a), Box::new(b))),
                Box::new(c),
            ),
        ] {
            assert_eq!(parse(&expr.to_string()).unwrap(), expr);
        }
    }
}
