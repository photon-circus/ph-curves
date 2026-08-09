//! Simple recursive-descent math expression evaluator.
//!
//! Evaluates a formula string in terms of `t` (0.0..=1.0).
//!
//! # Grammar
//!
//! ```text
//! expression = term (('+' | '-') term)*
//! term       = power (('*' | '/') power)*
//! power      = unary ('^' unary)*          (right-associative)
//! unary      = '-' unary | atom
//! atom       = NUMBER | IDENT | IDENT '(' args ')' | '(' expression ')'
//! args       = expression (',' expression)*
//! ```
//!
//! Supported functions: `pow`, `sqrt`, `abs`, `min`, `max`, `clamp`,
//! `sin`, `cos`, `tan`, `exp`, `ln`, `log2`.
//!
//! Constants: `pi`, `e`.

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
    Caret,
}

fn tokenize(expr: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            '+' => {
                tokens.push(Token::Plus);
                chars.next();
            }
            '-' => {
                tokens.push(Token::Minus);
                chars.next();
            }
            '*' if chars.clone().nth(1) == Some('*') => {
                tokens.push(Token::Caret);
                chars.next();
                chars.next();
            }
            '*' => {
                tokens.push(Token::Star);
                chars.next();
            }
            '/' => {
                tokens.push(Token::Slash);
                chars.next();
            }
            '^' => {
                tokens.push(Token::Caret);
                chars.next();
            }
            '(' => {
                tokens.push(Token::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(Token::RParen);
                chars.next();
            }
            ',' => {
                tokens.push(Token::Comma);
                chars.next();
            }
            c if c.is_ascii_digit() || c == '.' => {
                let mut num_str = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        num_str.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Num(
                    num_str
                        .parse::<f64>()
                        .unwrap_or_else(|_| panic!("invalid number `{num_str}`")),
                ));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut ident = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_alphanumeric() || d == '_' {
                        ident.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Ident(ident));
            }
            other => panic!("unexpected character `{other}` in formula"),
        }
    }
    tokens
}

// ---------------------------------------------------------------------------
// Recursive-descent evaluator
// ---------------------------------------------------------------------------

struct Evaluator<'a> {
    tokens: &'a [Token],
    pos: usize,
    variable_name: &'a str,
    variable_value: f64,
}

impl<'a> Evaluator<'a> {
    fn new(tokens: &'a [Token], variable_name: &'a str, variable_value: f64) -> Self {
        Self {
            tokens,
            pos: 0,
            variable_name,
            variable_value,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next_token(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) {
        let got = self.next_token();
        assert!(got == Some(expected), "expected {expected:?}, got {got:?}");
    }

    // ── Grammar rules ──────────────────────────────────────────────────

    fn expr(&mut self) -> f64 {
        let mut left = self.term();
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.next_token();
                    left += self.term();
                }
                Some(Token::Minus) => {
                    self.next_token();
                    left -= self.term();
                }
                _ => break,
            }
        }
        left
    }

    fn term(&mut self) -> f64 {
        let mut left = self.power();
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.next_token();
                    left *= self.power();
                }
                Some(Token::Slash) => {
                    self.next_token();
                    left /= self.power();
                }
                _ => break,
            }
        }
        left
    }

    fn power(&mut self) -> f64 {
        let base = self.unary();
        if let Some(Token::Caret) = self.peek() {
            self.next_token();
            let exp = self.power(); // right-associative
            base.powf(exp)
        } else {
            base
        }
    }

    fn unary(&mut self) -> f64 {
        if let Some(Token::Minus) = self.peek() {
            self.next_token();
            -self.unary()
        } else {
            self.atom()
        }
    }

    fn atom(&mut self) -> f64 {
        match self.next_token().cloned() {
            Some(Token::Num(n)) => n,
            Some(Token::LParen) => {
                let val = self.expr();
                self.expect(&Token::RParen);
                val
            }
            Some(Token::Ident(name)) => {
                if self.peek() == Some(&Token::LParen) {
                    self.call_func(&name)
                } else {
                    self.resolve_var(&name)
                }
            }
            other => panic!("unexpected token {other:?} in formula"),
        }
    }

    // ── Variables & functions ──────────────────────────────────────────

    fn resolve_var(&self, name: &str) -> f64 {
        match name {
            "pi" | "PI" => std::f64::consts::PI,
            "e" | "E" => std::f64::consts::E,
            variable if variable == self.variable_name => self.variable_value,
            other => {
                panic!(
                    "unknown variable `{other}` in formula (only `{}`, `pi`, `e` allowed)",
                    self.variable_name
                )
            }
        }
    }

    fn parse_args(&mut self) -> Vec<f64> {
        self.expect(&Token::LParen);
        let mut args = Vec::new();
        if self.peek() != Some(&Token::RParen) {
            args.push(self.expr());
            while self.peek() == Some(&Token::Comma) {
                self.next_token();
                args.push(self.expr());
            }
        }
        self.expect(&Token::RParen);
        args
    }

    fn call_func(&mut self, name: &str) -> f64 {
        let args = self.parse_args();
        match name {
            "pow" => {
                assert!(args.len() == 2, "pow() takes 2 arguments");
                args[0].powf(args[1])
            }
            "sqrt" => {
                assert!(args.len() == 1, "sqrt() takes 1 argument");
                args[0].sqrt()
            }
            "abs" => {
                assert!(args.len() == 1, "abs() takes 1 argument");
                args[0].abs()
            }
            "sin" => {
                assert!(args.len() == 1, "sin() takes 1 argument");
                args[0].sin()
            }
            "cos" => {
                assert!(args.len() == 1, "cos() takes 1 argument");
                args[0].cos()
            }
            "tan" => {
                assert!(args.len() == 1, "tan() takes 1 argument");
                args[0].tan()
            }
            "exp" => {
                assert!(args.len() == 1, "exp() takes 1 argument");
                args[0].exp()
            }
            "ln" => {
                assert!(args.len() == 1, "ln() takes 1 argument");
                args[0].ln()
            }
            "log2" => {
                assert!(args.len() == 1, "log2() takes 1 argument");
                args[0].log2()
            }
            "min" => {
                assert!(args.len() == 2, "min() takes 2 arguments");
                args[0].min(args[1])
            }
            "max" => {
                assert!(args.len() == 2, "max() takes 2 arguments");
                args[0].max(args[1])
            }
            "clamp" => {
                assert!(args.len() == 3, "clamp() takes 3 arguments");
                args[0].clamp(args[1], args[2])
            }
            other => panic!("unknown function `{other}()` in formula"),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A parsed formula that can be evaluated repeatedly without tokenizing again.
pub struct Formula {
    tokens: Vec<Token>,
}

impl Formula {
    /// Tokenize an expression for repeated evaluation.
    ///
    /// Evaluation performs syntax and semantic validation because some valid
    /// operations depend on the caller-provided variable value.
    pub fn parse(expr: &str) -> Self {
        let tokens = tokenize(expr);
        Self { tokens }
    }

    /// Evaluate the expression using `variable_name = variable_value`.
    pub fn eval(&self, variable_name: &str, variable_value: f64) -> f64 {
        let mut evaluator = Evaluator::new(&self.tokens, variable_name, variable_value);
        let result = evaluator.expr();
        assert!(
            evaluator.pos == self.tokens.len(),
            "trailing tokens in formula: {:?}",
            &self.tokens[evaluator.pos..]
        );
        result
    }
}

/// Evaluate `expr` for a given `t` value (0.0..=1.0).
#[cfg(test)]
pub fn eval(expr: &str, t: f64) -> f64 {
    Formula::parse(expr).eval("t", t)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{E, PI};

    /// Helper: assert that `eval(expr, t)` is within `eps` of `expected`.
    fn assert_close(expr: &str, t: f64, expected: f64, eps: f64) {
        let got = eval(expr, t);
        assert!(
            (got - expected).abs() < eps,
            "eval({expr:?}, {t}) = {got}, expected {expected} (eps={eps})"
        );
    }

    /// Default tolerance for most comparisons.
    const EPS: f64 = 1e-12;

    // ── Literals & constants ──────────────────────────────────────────

    #[test]
    fn literal_integer() {
        assert_close("42", 0.0, 42.0, EPS);
    }

    #[test]
    fn literal_float() {
        assert_close("1.23", 0.0, 1.23, EPS);
    }

    #[test]
    fn literal_leading_dot() {
        assert_close(".5", 0.0, 0.5, EPS);
    }

    #[test]
    fn constant_pi() {
        assert_close("pi", 0.0, PI, EPS);
    }

    #[test]
    fn constant_pi_uppercase() {
        assert_close("PI", 0.0, PI, EPS);
    }

    #[test]
    fn constant_e() {
        assert_close("e", 0.0, E, EPS);
    }

    #[test]
    fn constant_e_uppercase() {
        assert_close("E", 0.0, E, EPS);
    }

    // ── Variable t ────────────────────────────────────────────────────

    #[test]
    fn variable_t_zero() {
        assert_close("t", 0.0, 0.0, EPS);
    }

    #[test]
    fn variable_t_one() {
        assert_close("t", 1.0, 1.0, EPS);
    }

    #[test]
    fn variable_t_mid() {
        assert_close("t", 0.37, 0.37, EPS);
    }

    // ── Arithmetic operators ──────────────────────────────────────────

    #[test]
    fn addition() {
        assert_close("1 + 2", 0.0, 3.0, EPS);
    }

    #[test]
    fn subtraction() {
        assert_close("5 - 3", 0.0, 2.0, EPS);
    }

    #[test]
    fn multiplication() {
        assert_close("3 * 4", 0.0, 12.0, EPS);
    }

    #[test]
    fn division() {
        assert_close("10 / 4", 0.0, 2.5, EPS);
    }

    #[test]
    fn caret_power() {
        assert_close("2 ^ 10", 0.0, 1024.0, EPS);
    }

    #[test]
    fn double_star_power() {
        assert_close("2 ** 10", 0.0, 1024.0, EPS);
    }

    #[test]
    fn unary_minus() {
        assert_close("-3", 0.0, -3.0, EPS);
    }

    #[test]
    fn double_unary_minus() {
        assert_close("--3", 0.0, 3.0, EPS);
    }

    #[test]
    fn unary_minus_on_variable() {
        assert_close("-t", 0.7, -0.7, EPS);
    }

    // ── Operator precedence ───────────────────────────────────────────

    #[test]
    fn mul_before_add() {
        // 2 + 3*4 = 14, not 20
        assert_close("2 + 3 * 4", 0.0, 14.0, EPS);
    }

    #[test]
    fn div_before_sub() {
        // 10 - 6/2 = 7, not 2
        assert_close("10 - 6 / 2", 0.0, 7.0, EPS);
    }

    #[test]
    fn power_before_mul() {
        // 2 * 3^2 = 18, not 36
        assert_close("2 * 3 ^ 2", 0.0, 18.0, EPS);
    }

    #[test]
    fn power_right_associative() {
        // 2^3^2 = 2^(3^2) = 2^9 = 512, not (2^3)^2 = 64
        assert_close("2^3^2", 0.0, 512.0, EPS);
    }

    #[test]
    fn unary_minus_precedence() {
        // In our grammar, unary minus binds tighter than ^:
        // -2^2 = (-2)^2 = 4.   Use -(2^2) for the other interpretation.
        assert_close("-2^2", 0.0, 4.0, EPS);
        assert_close("-(2^2)", 0.0, -4.0, EPS);
    }

    #[test]
    fn parentheses_override_precedence() {
        assert_close("(2 + 3) * 4", 0.0, 20.0, EPS);
    }

    #[test]
    fn nested_parentheses() {
        assert_close("((1 + 2) * (3 + 4))", 0.0, 21.0, EPS);
    }

    // ── Whitespace handling ───────────────────────────────────────────

    #[test]
    fn no_whitespace() {
        assert_close("1+2*3", 0.0, 7.0, EPS);
    }

    #[test]
    fn excessive_whitespace() {
        assert_close("  1  +  2  *  3  ", 0.0, 7.0, EPS);
    }

    #[test]
    fn tabs_and_newlines() {
        assert_close("1\t+\n2", 0.0, 3.0, EPS);
    }

    // ── Functions: pow ────────────────────────────────────────────────

    #[test]
    fn pow_integer_exponent() {
        assert_close("pow(2, 8)", 0.0, 256.0, EPS);
    }

    #[test]
    fn pow_fractional_exponent() {
        assert_close("pow(t, 2.2)", 0.5, 0.5_f64.powf(2.2), EPS);
    }

    #[test]
    fn pow_zero_base() {
        assert_close("pow(0, 5)", 0.0, 0.0, EPS);
    }

    #[test]
    fn pow_zero_exponent() {
        assert_close("pow(123, 0)", 0.0, 1.0, EPS);
    }

    // ── Functions: sqrt ───────────────────────────────────────────────

    #[test]
    fn sqrt_perfect() {
        assert_close("sqrt(9)", 0.0, 3.0, EPS);
    }

    #[test]
    fn sqrt_of_t() {
        assert_close("sqrt(t)", 0.25, 0.5, EPS);
    }

    // ── Functions: abs ────────────────────────────────────────────────

    #[test]
    fn abs_positive() {
        assert_close("abs(3)", 0.0, 3.0, EPS);
    }

    #[test]
    fn abs_negative() {
        assert_close("abs(-7)", 0.0, 7.0, EPS);
    }

    #[test]
    fn abs_zero() {
        assert_close("abs(0)", 0.0, 0.0, EPS);
    }

    // ── Functions: trig ───────────────────────────────────────────────

    #[test]
    fn sin_zero() {
        assert_close("sin(0)", 0.0, 0.0, EPS);
    }

    #[test]
    fn sin_pi_half() {
        assert_close("sin(pi / 2)", 0.0, 1.0, EPS);
    }

    #[test]
    fn cos_zero() {
        assert_close("cos(0)", 0.0, 1.0, EPS);
    }

    #[test]
    fn cos_pi() {
        assert_close("cos(pi)", 0.0, -1.0, EPS);
    }

    #[test]
    fn tan_zero() {
        assert_close("tan(0)", 0.0, 0.0, EPS);
    }

    #[test]
    fn sin_of_t_times_pi() {
        // sin(0.5 * pi) = 1
        assert_close("sin(t * pi)", 0.5, 1.0, EPS);
    }

    // ── Functions: exp / ln / log2 ────────────────────────────────────

    #[test]
    fn exp_zero() {
        assert_close("exp(0)", 0.0, 1.0, EPS);
    }

    #[test]
    fn exp_one() {
        assert_close("exp(1)", 0.0, E, EPS);
    }

    #[test]
    fn ln_one() {
        assert_close("ln(1)", 0.0, 0.0, EPS);
    }

    #[test]
    fn ln_e() {
        assert_close("ln(e)", 0.0, 1.0, EPS);
    }

    #[test]
    fn log2_eight() {
        assert_close("log2(8)", 0.0, 3.0, EPS);
    }

    #[test]
    fn log2_one() {
        assert_close("log2(1)", 0.0, 0.0, EPS);
    }

    #[test]
    fn exp_ln_roundtrip() {
        assert_close("ln(exp(3.7))", 0.0, 3.7, EPS);
    }

    // ── Functions: min / max / clamp ──────────────────────────────────

    #[test]
    fn min_picks_smaller() {
        assert_close("min(3, 7)", 0.0, 3.0, EPS);
    }

    #[test]
    fn min_with_t() {
        assert_close("min(t, 0.5)", 0.8, 0.5, EPS);
    }

    #[test]
    fn max_picks_larger() {
        assert_close("max(3, 7)", 0.0, 7.0, EPS);
    }

    #[test]
    fn max_with_t() {
        assert_close("max(t, 0.5)", 0.2, 0.5, EPS);
    }

    #[test]
    fn clamp_within_range() {
        assert_close("clamp(0.5, 0, 1)", 0.0, 0.5, EPS);
    }

    #[test]
    fn clamp_below_range() {
        assert_close("clamp(-1, 0, 1)", 0.0, 0.0, EPS);
    }

    #[test]
    fn clamp_above_range() {
        assert_close("clamp(5, 0, 1)", 0.0, 1.0, EPS);
    }

    // ── Compound / real-world formulas ────────────────────────────────

    #[test]
    fn gamma_22_at_zero() {
        assert_close("pow(t, 2.2)", 0.0, 0.0, EPS);
    }

    #[test]
    fn gamma_22_at_one() {
        assert_close("pow(t, 2.2)", 1.0, 1.0, EPS);
    }

    #[test]
    fn gamma_22_mid() {
        assert_close("pow(t, 2.2)", 0.5, 0.5_f64.powf(2.2), EPS);
    }

    #[test]
    fn inverse_gamma() {
        assert_close("pow(t, 1.0 / 2.2)", 0.5, 0.5_f64.powf(1.0 / 2.2), EPS);
    }

    #[test]
    fn smoothstep_formula() {
        // 3t^2 - 2t^3 at t=0.5 => 0.5
        assert_close("3 * t^2 - 2 * t^3", 0.5, 0.5, EPS);
    }

    #[test]
    fn smoothstep_endpoints() {
        assert_close("3 * t^2 - 2 * t^3", 0.0, 0.0, EPS);
        assert_close("3 * t^2 - 2 * t^3", 1.0, 1.0, EPS);
    }

    #[test]
    fn smoother_step_formula() {
        // 6t^5 - 15t^4 + 10t^3
        let t: f64 = 0.3;
        let expected = 6.0 * t.powi(5) - 15.0 * t.powi(4) + 10.0 * t.powi(3);
        assert_close("6*t^5 - 15*t^4 + 10*t^3", t, expected, EPS);
    }

    #[test]
    fn sine_ease_in_out() {
        // (1 - cos(t * pi)) / 2
        let t = 0.25;
        let expected = (1.0 - (t * PI).cos()) / 2.0;
        assert_close("(1 - cos(t * pi)) / 2", t, expected, EPS);
    }

    #[test]
    fn power_s_curve() {
        // t^2 / (t^2 + (1-t)^2)
        let t = 0.3;
        let expected = t * t / (t * t + (1.0 - t) * (1.0 - t));
        assert_close("t^2 / (t^2 + (1 - t)^2)", t, expected, EPS);
    }

    #[test]
    fn cie_lightness() {
        let t = 0.5;
        let expected = ((t + 0.16) / 1.16_f64).powi(3);
        assert_close("pow((t + 0.16) / 1.16, 3.0)", t, expected, EPS);
    }

    #[test]
    fn nested_function_calls() {
        // sqrt(abs(-t))
        assert_close("sqrt(abs(-t))", 0.64, 0.8, EPS);
    }

    #[test]
    fn function_with_expression_args() {
        // pow(t + 0.1, 2 * 1.1)
        let t = 0.4;
        let expected = (t + 0.1_f64).powf(2.0 * 1.1);
        assert_close("pow(t + 0.1, 2 * 1.1)", t, expected, EPS);
    }

    #[test]
    fn complex_expression() {
        // max(0, min(1, (t - 0.2) / 0.6))  — a linear ramp clamped to 0..1
        assert_close("max(0, min(1, (t - 0.2) / 0.6))", 0.0, 0.0, EPS);
        assert_close("max(0, min(1, (t - 0.2) / 0.6))", 0.5, 0.5, EPS);
        assert_close("max(0, min(1, (t - 0.2) / 0.6))", 1.0, 1.0, EPS);
    }

    // ── Tokenizer edge cases ──────────────────────────────────────────

    #[test]
    fn tokenize_empty() {
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn tokenize_all_operators() {
        let tokens = tokenize("+ - * / ^ ( ) ,");
        assert_eq!(
            tokens,
            vec![
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Caret,
                Token::LParen,
                Token::RParen,
                Token::Comma,
            ]
        );
    }

    #[test]
    fn tokenize_double_star() {
        let tokens = tokenize("**");
        assert_eq!(tokens, vec![Token::Caret]);
    }

    #[test]
    fn tokenize_ident_with_underscores() {
        let tokens = tokenize("my_var_2");
        assert_eq!(tokens, vec![Token::Ident("my_var_2".to_string())]);
    }

    #[test]
    fn tokenize_number_then_ident() {
        // "2t" should be two tokens, not one
        let tokens = tokenize("2t");
        assert_eq!(tokens, vec![Token::Num(2.0), Token::Ident("t".to_string())]);
    }

    // ── Error cases ───────────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "unknown variable")]
    fn unknown_variable_panics() {
        eval("x", 0.0);
    }

    #[test]
    #[should_panic(expected = "unknown function")]
    fn unknown_function_panics() {
        eval("foo(1)", 0.0);
    }

    #[test]
    #[should_panic(expected = "unexpected character")]
    fn invalid_character_panics() {
        eval("t @ 2", 0.0);
    }

    #[test]
    #[should_panic(expected = "trailing tokens")]
    fn trailing_tokens_panics() {
        eval("1 2", 0.0);
    }

    #[test]
    #[should_panic(expected = "expected")]
    fn missing_close_paren_panics() {
        eval("(1 + 2", 0.0);
    }

    #[test]
    #[should_panic(expected = "takes 2 arguments")]
    fn pow_wrong_arity_panics() {
        eval("pow(1)", 0.0);
    }

    #[test]
    #[should_panic(expected = "takes 1 argument")]
    fn sqrt_wrong_arity_panics() {
        eval("sqrt(1, 2)", 0.0);
    }

    #[test]
    #[should_panic(expected = "takes 3 arguments")]
    fn clamp_wrong_arity_panics() {
        eval("clamp(1, 2)", 0.0);
    }

    #[test]
    #[should_panic(expected = "unexpected token")]
    fn empty_expression_panics() {
        eval("", 0.0);
    }
}
