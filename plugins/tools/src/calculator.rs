#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Symbol(char),
}
struct Parser {
    tokens: Vec<Token>,
    offset: usize,
    depth: usize,
}
impl Parser {
    fn symbol(&mut self, symbol: char) -> bool {
        if self.tokens.get(self.offset) == Some(&Token::Symbol(symbol)) {
            self.offset += 1;
            true
        } else {
            false
        }
    }
    fn expression(&mut self, minimum: u8) -> Result<f64, String> {
        self.depth += 1;
        if self.depth > 32 {
            return Err("expression nesting is too deep".into());
        }
        let mut value = if self.symbol('-') {
            -self.expression(6)?
        } else if self.symbol('+') {
            self.expression(6)?
        } else if self.symbol('(') {
            let value = self.expression(0)?;
            if !self.symbol(')') {
                return Err("expected closing parenthesis".into());
            }
            value
        } else {
            let token = self
                .tokens
                .get(self.offset)
                .cloned()
                .ok_or_else(|| "incomplete expression".to_owned())?;
            self.offset += 1;
            match token {
                Token::Number(value) => value,
                Token::Name(name) if name == "pi" => std::f64::consts::PI,
                Token::Name(name) if name == "e" => std::f64::consts::E,
                Token::Name(name) => {
                    if !self.symbol('(') {
                        return Err("expected function argument".into());
                    }
                    let argument = self.expression(0)?;
                    let second = if self.symbol(',') {
                        Some(self.expression(0)?)
                    } else {
                        None
                    };
                    if !self.symbol(')') {
                        return Err("expected closing parenthesis".into());
                    }
                    match (name.as_str(), second) {
                        ("sqrt", None) => argument.sqrt(),
                        ("abs", None) => argument.abs(),
                        ("sin", None) => argument.sin(),
                        ("cos", None) => argument.cos(),
                        ("tan", None) => argument.tan(),
                        ("ln", None) => argument.ln(),
                        ("log", None) => argument.log10(),
                        ("round", None) => argument.round(),
                        ("floor", None) => argument.floor(),
                        ("ceil", None) => argument.ceil(),
                        ("min", Some(other)) => argument.min(other),
                        ("max", Some(other)) => argument.max(other),
                        _ => return Err("unknown function or argument count".into()),
                    }
                }
                _ => return Err("expected number".into()),
            }
        };
        while let Some(Token::Symbol(operator)) = self.tokens.get(self.offset).cloned() {
            let (left, right) = match operator {
                '+' | '-' => (2, 3),
                '*' | '/' | '%' => (4, 5),
                '^' => (7, 7),
                _ => break,
            };
            if left < minimum {
                break;
            }
            self.offset += 1;
            let other = self.expression(right)?;
            value = match operator {
                '+' => value + other,
                '-' => value - other,
                '*' => value * other,
                '/' => value / other,
                '%' => value % other,
                '^' => value.powf(other),
                _ => unreachable!(),
            };
            if !value.is_finite() {
                return Err("result is not finite".into());
            }
        }
        self.depth -= 1;
        if value.is_finite() {
            Ok(value)
        } else {
            Err("result is not finite".into())
        }
    }
}
pub fn calculate(input: &str) -> Result<String, String> {
    if input.is_empty() || input.len() > 1024 || !input.is_ascii() {
        return Err("expected a bounded arithmetic expression".into());
    }
    let bytes = input.as_bytes();
    let mut offset = 0;
    let mut tokens = Vec::new();
    while offset < bytes.len() {
        if bytes[offset].is_ascii_whitespace() {
            offset += 1;
            continue;
        }
        let start = offset;
        let token = if bytes[offset].is_ascii_digit() || bytes[offset] == b'.' {
            offset += 1;
            while offset < bytes.len()
                && (bytes[offset].is_ascii_digit()
                    || bytes[offset] == b'.'
                    || matches!(bytes[offset], b'e' | b'E')
                    || (matches!(bytes[offset], b'+' | b'-')
                        && matches!(bytes[offset - 1], b'e' | b'E')))
            {
                offset += 1;
            }
            Token::Number(
                input[start..offset]
                    .parse()
                    .map_err(|_| "invalid number".to_owned())?,
            )
        } else if bytes[offset].is_ascii_alphabetic() {
            offset += 1;
            while offset < bytes.len() && bytes[offset].is_ascii_alphabetic() {
                offset += 1;
            }
            Token::Name(input[start..offset].to_ascii_lowercase())
        } else {
            let symbol = bytes[offset] as char;
            offset += 1;
            if !"+-*/%^(),".contains(symbol) {
                return Err("unsupported operator".into());
            }
            Token::Symbol(symbol)
        };
        tokens.push(token);
        if tokens.len() > 128 {
            return Err("too many expression tokens".into());
        }
    }
    let mut parser = Parser {
        tokens,
        offset: 0,
        depth: 0,
    };
    let value = parser.expression(0)?;
    if parser.offset != parser.tokens.len() {
        return Err("unexpected trailing input".into());
    }
    if value.abs() >= 1e15 || (value != 0.0 && value.abs() < 1e-8) {
        Ok(format!("{value:.8e}"))
    } else {
        let result = format!("{value:.8}");
        Ok(result
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned())
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn precedence_functions_and_invalid_results() {
        assert_eq!(super::calculate("sqrt(9) + 2^3^2").unwrap(), "515");
        assert_eq!(super::calculate("-3^2 + max(2, 4)").unwrap(), "-5");
        assert_eq!(super::calculate("1e-3 * 1000").unwrap(), "1");
        for expression in ["1/0", "sqrt(-1)", "foo(2)", "2 3", "2+", "NaN"] {
            assert!(super::calculate(expression).is_err(), "{expression}");
        }
        assert!(super::calculate(&"(".repeat(40)).is_err());
    }
}
