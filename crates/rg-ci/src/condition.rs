//! Safe, deliberately small evaluator for static CI `if` expressions.

use anyhow::{bail, Context, Result};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Bool(bool),
    String(String),
}
impl Value {
    fn truthy(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::String(value) => !value.is_empty(),
        }
    }
    fn text(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::String(value) => value.clone(),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Ident(String),
    String(String),
    Bool(bool),
    Eq,
    Ne,
    And,
    Or,
    Not,
    LParen,
    RParen,
    Comma,
    End,
}

pub fn validate_condition(input: &str) -> Result<()> {
    evaluate_with(input, |name| {
        if matches!(
            name,
            "github.ref" | "github.ref_name" | "github.event_name" | "github.sha"
        ) || name.strip_prefix("env.").is_some_and(valid_context_key)
            || name.strip_prefix("matrix.").is_some_and(valid_context_key)
            // `steps.<id>.outputs.<name>` and `needs.<job>.outputs.<name>` are
            // accepted here even though this evaluator cannot resolve them
            // yet: a workflow that *uses* the syntax is valid, and rejecting it
            // outright made every such workflow fail to parse. The values
            // resolve at runtime (or to empty, per GitHub's semantics).
            || is_step_or_need_output(name)
        {
            Some(String::new())
        } else {
            None
        }
    })
    .map(|_| ())
}

/// `steps.<id>.outputs.<name>` or `needs.<job>.outputs.<name>`.
///
/// Shape-checked rather than prefix-checked: `steps.anything` is not a context
/// this engine will ever provide, and silently accepting a typo would turn a
/// mistake into an always-empty value.
fn is_step_or_need_output(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("steps.")
        .or_else(|| name.strip_prefix("needs."))
    else {
        return false;
    };
    let mut parts = rest.split('.');
    let Some(handle) = parts.next() else {
        return false;
    };
    if handle.is_empty() || !valid_context_key(handle) {
        return false;
    }
    parts.next() == Some("outputs") && parts.next().is_some_and(valid_context_key)
}

pub fn evaluate_condition(input: &str, context: &HashMap<String, String>) -> Result<bool> {
    evaluate_with(input, |name| {
        context.get(name).cloned().or_else(|| {
            if name.starts_with("env.") || name.starts_with("matrix.") {
                return Some(String::new());
            }
            // GitHub's semantics: an output that does not exist is empty, not
            // an error. `needs.x.outputs.y == ''` is therefore true when x has
            // not produced y — which is what a workflow branching on an
            // optional value expects.
            if is_step_or_need_output(name) {
                return Some(String::new());
            }
            None
        })
    })
}

fn valid_context_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn evaluate_with(input: &str, resolver: impl Fn(&str) -> Option<String>) -> Result<bool> {
    let expression = unwrap_expression(input)?;
    let tokens = tokenize(expression)?;
    let mut parser = Parser {
        tokens,
        position: 0,
        resolver: &resolver,
    };
    let result = parser.parse_or()?;
    if parser.peek() != &Token::End {
        bail!("unexpected token after condition expression");
    }
    Ok(result.truthy())
}

fn unwrap_expression(input: &str) -> Result<&str> {
    let value = input.trim();
    if let Some(inner) = value.strip_prefix("${{") {
        return inner
            .strip_suffix("}}")
            .map(str::trim)
            .context("condition is missing closing '}}'");
    }
    Ok(value)
}

fn tokenize(input: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ch if ch.is_whitespace() => i += 1,
            '(' => {
                out.push(Token::LParen);
                i += 1;
            }
            ')' => {
                out.push(Token::RParen);
                i += 1;
            }
            ',' => {
                out.push(Token::Comma);
                i += 1;
            }
            '!' if chars.get(i + 1) == Some(&'=') => {
                out.push(Token::Ne);
                i += 2;
            }
            '!' => {
                out.push(Token::Not);
                i += 1;
            }
            '=' if chars.get(i + 1) == Some(&'=') => {
                out.push(Token::Eq);
                i += 2;
            }
            '&' if chars.get(i + 1) == Some(&'&') => {
                out.push(Token::And);
                i += 2;
            }
            '|' if chars.get(i + 1) == Some(&'|') => {
                out.push(Token::Or);
                i += 2;
            }
            quote @ ('\'' | '"') => {
                i += 1;
                let mut value = String::new();
                let mut closed = false;
                while i < chars.len() {
                    if chars[i] == quote {
                        i += 1;
                        closed = true;
                        break;
                    }
                    if chars[i] == '\\' {
                        i += 1;
                        if i >= chars.len() {
                            bail!("unterminated escape in condition string");
                        }
                    }
                    value.push(chars[i]);
                    i += 1;
                }
                if !closed {
                    bail!("unterminated condition string");
                }
                out.push(Token::String(value));
            }
            ch if ch.is_ascii_alphanumeric() || ch == '_' => {
                let start = i;
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.'))
                {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                match ident.as_str() {
                    "true" => out.push(Token::Bool(true)),
                    "false" => out.push(Token::Bool(false)),
                    _ => out.push(Token::Ident(ident)),
                }
            }
            ch => bail!("unsupported character '{ch}' in condition"),
        }
    }
    out.push(Token::End);
    Ok(out)
}

struct Parser<'a, F> {
    tokens: Vec<Token>,
    position: usize,
    resolver: &'a F,
}
impl<'a, F: Fn(&str) -> Option<String>> Parser<'a, F> {
    fn peek(&self) -> &Token {
        self.tokens.get(self.position).unwrap_or(&Token::End)
    }
    fn take(&mut self) -> Token {
        let token = self.peek().clone();
        self.position += 1;
        token
    }
    fn parse_or(&mut self) -> Result<Value> {
        let mut left = self.parse_and()?;
        while self.peek() == &Token::Or {
            self.take();
            let right = self.parse_and()?;
            left = Value::Bool(left.truthy() || right.truthy());
        }
        Ok(left)
    }
    fn parse_and(&mut self) -> Result<Value> {
        let mut left = self.parse_equality()?;
        while self.peek() == &Token::And {
            self.take();
            let right = self.parse_equality()?;
            left = Value::Bool(left.truthy() && right.truthy());
        }
        Ok(left)
    }
    fn parse_equality(&mut self) -> Result<Value> {
        let left = self.parse_unary()?;
        match self.peek() {
            Token::Eq => {
                self.take();
                let right = self.parse_unary()?;
                Ok(Value::Bool(left == right || left.text() == right.text()))
            }
            Token::Ne => {
                self.take();
                let right = self.parse_unary()?;
                Ok(Value::Bool(!(left == right || left.text() == right.text())))
            }
            _ => Ok(left),
        }
    }
    fn parse_unary(&mut self) -> Result<Value> {
        if self.peek() == &Token::Not {
            self.take();
            return Ok(Value::Bool(!self.parse_unary()?.truthy()));
        }
        self.parse_primary()
    }
    fn parse_primary(&mut self) -> Result<Value> {
        match self.take() {
            Token::Bool(value) => Ok(Value::Bool(value)),
            Token::String(value) => Ok(Value::String(value)),
            Token::LParen => {
                let value = self.parse_or()?;
                if self.take() != Token::RParen {
                    bail!("condition is missing ')'");
                }
                Ok(value)
            }
            Token::Ident(name) if self.peek() == &Token::LParen => self.parse_function(&name),
            Token::Ident(name) => (self.resolver)(&name)
                .map(Value::String)
                .with_context(|| format!("unsupported or unavailable condition context '{name}'")),
            token => bail!("unexpected token in condition: {token:?}"),
        }
    }
    fn parse_function(&mut self, name: &str) -> Result<Value> {
        self.take();
        let lower = name.to_ascii_lowercase();
        // Both resolve to "run" here: this evaluator runs when the pipeline is
        // created, where nothing has failed yet. `always()` differs from
        // `success()` not here but in the scheduling rules — a job that asks
        // for `always()` is allowed to start after a failed stage, which is
        // what makes cleanup and notification jobs possible.
        if matches!(lower.as_str(), "success" | "always") {
            if self.take() != Token::RParen {
                bail!("{name}() does not accept arguments");
            }
            return Ok(Value::Bool(true));
        }
        if !matches!(lower.as_str(), "startswith" | "endswith" | "contains") {
            bail!("unsupported condition function '{name}'");
        }
        let left = self.parse_or()?;
        if self.take() != Token::Comma {
            bail!("{name}() requires two arguments");
        }
        let right = self.parse_or()?;
        if self.take() != Token::RParen {
            bail!("{name}() requires two arguments");
        }
        let (left, right) = (left.text(), right.text());
        Ok(Value::Bool(match lower.as_str() {
            "startswith" => left.starts_with(&right),
            "endswith" => left.ends_with(&right),
            _ => left.contains(&right),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evaluates_boolean_context_and_string_functions() {
        let context = HashMap::from([
            ("github.ref".into(), "refs/heads/main".into()),
            ("env.DEPLOY".into(), "yes".into()),
        ]);
        assert!(evaluate_condition(
            "github.ref == 'refs/heads/main' && env.DEPLOY == 'yes'",
            &context
        )
        .unwrap());
        assert!(evaluate_condition(
            "${{ startsWith(github.ref, 'refs/heads/') && !false }}",
            &context
        )
        .unwrap());
        assert!(!evaluate_condition("contains(github.ref, 'tags') || false", &context).unwrap());
    }
    /// A workflow that branches on an output is valid, and an output that does
    /// not exist reads as empty — GitHub's semantics, and the reason this had
    /// to stop being an error: rejecting the syntax made every such workflow
    /// fail to parse at all.
    #[test]
    fn step_and_job_outputs_are_accepted_and_default_to_empty() {
        assert!(validate_condition("needs.build.outputs.digest != ''").is_ok());
        assert!(validate_condition("steps.pack.outputs.sha == 'abc'").is_ok());

        let empty = HashMap::new();
        assert!(!evaluate_condition("needs.build.outputs.digest != ''", &empty).unwrap());
        assert!(evaluate_condition("needs.build.outputs.digest == ''", &empty).unwrap());

        let mut context = HashMap::new();
        context.insert(
            "needs.build.outputs.digest".to_string(),
            "sha256:abc".to_string(),
        );
        assert!(evaluate_condition("needs.build.outputs.digest != ''", &context).unwrap());
        assert!(evaluate_condition(
            "steps.pack.outputs.sha == 'abc'",
            &HashMap::from([("steps.pack.outputs.sha".to_string(), "abc".to_string())])
        )
        .unwrap());
    }

    /// `always()` is accepted and evaluates to "run": at creation time nothing
    /// has failed, and the difference from `success()` lives in the scheduling
    /// rules, not here.
    #[test]
    fn always_is_accepted_and_means_run() {
        assert!(validate_condition("always()").is_ok());
        assert!(evaluate_condition("always()", &HashMap::new()).unwrap());
        // Combined with a real context entry, as the pipeline provides it.
        let context = HashMap::from([("github.ref".to_string(), "refs/heads/main".to_string())]);
        assert!(evaluate_condition("always() && github.ref != ''", &context).unwrap());
        // Still not a general escape hatch: the other runtime functions are
        // refused until their scheduling semantics exist.
        assert!(validate_condition("cancelled()").is_err());
        assert!(validate_condition("always").is_err());
    }

    /// Shape-checked, not prefix-checked: `steps.pack` is not a context this
    /// engine provides, and treating a typo as "empty" would hide the mistake.
    #[test]
    fn a_bare_step_or_need_reference_is_still_rejected() {
        assert!(validate_condition("steps.pack == 'x'").is_err());
        assert!(validate_condition("needs.build.outputs == 'x'").is_err());
        assert!(validate_condition("needs.build.inputs.x == 'x'").is_err());
    }

    #[test]
    fn rejects_unknown_context_functions_and_syntax() {
        assert!(validate_condition("secrets.TOKEN == 'x'").is_err());
        assert!(validate_condition("hashFiles('**')").is_err());
        assert!(validate_condition("failure()").is_err());
        assert!(validate_condition("github.ref ==").is_err());
    }
}
