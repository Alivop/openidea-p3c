//! 常量定义

use super::Ctx;
use crate::lex::Kind;
use openidea_plugin::{Diagnostic, Severity};

/// 手册·常量定义 2：long 字面量后缀必须是大写 L，小写 l 易与数字 1 混淆。
pub fn long_literal_suffix(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for t in &ctx.code {
        if t.kind != Kind::Number {
            continue;
        }
        let s = ctx.slice(t);
        if !s.ends_with('l') || !is_integer_literal(s) {
            continue;
        }
        out.push(ctx.diag(
            t.end - 1,
            t.end,
            Severity::Warning,
            ctx.t(
                "long 字面量后缀应使用大写 L，小写 l 容易与数字 1 混淆（手册·常量定义 2）",
                "Use an uppercase L for the long literal suffix: a lowercase l is easily confused with the digit 1 \
                 (Handbook: Constants 2)",
            ),
        ));
    }
    out
}

/// Only integer literals take the `l` suffix; a hex float uses `p`, a decimal one `e`.
fn is_integer_literal(s: &str) -> bool {
    let body = &s[..s.len() - 1];
    let hex = body.starts_with("0x") || body.starts_with("0X");
    let exponent = if hex { |c| c == 'p' || c == 'P' } else { |c| c == 'e' || c == 'E' };
    !body.contains('.') && !body.contains(exponent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn hits(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        long_literal_suffix(&Ctx::new(src))
    }

    #[test]
    fn flags_the_lowercase_suffix_only() {
        assert_eq!(hits("long a = 2l;").len(), 1);
        assert_eq!(hits("long a = 2L;").len(), 0);
        assert_eq!(hits("long a = 1_000l;").len(), 1);
        assert_eq!(hits("long a = 0x1Fl;").len(), 1);
        assert_eq!(hits("long a = 0b101l;").len(), 1);
    }

    #[test]
    fn the_range_covers_just_the_suffix() {
        let src = "long a = 2l;";
        let d = &hits(src)[0];
        assert_eq!(&src[..d.range.start.character as usize], "long a = 2");
        assert_eq!(d.range.end.character - d.range.start.character, 1);
    }

    #[test]
    fn ignores_literals_in_strings_and_comments() {
        assert_eq!(hits(r#"String s = "2l"; // 3l"#).len(), 0);
        assert_eq!(hits("/* 2l */ char c = 'l';").len(), 0);
        assert_eq!(hits("String s = \"\"\"\n  2l\n  \"\"\";").len(), 0);
    }

    #[test]
    fn an_identifier_named_l_is_not_a_literal() {
        assert_eq!(hits("int l = 2; long a = l;").len(), 0);
    }

    #[test]
    fn floating_point_suffixes_are_untouched() {
        assert_eq!(hits("float f = 1.5f; double d = 2d; double e = 1e3;").len(), 0);
    }
}
