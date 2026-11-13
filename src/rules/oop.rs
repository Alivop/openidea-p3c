//! OOP 规约（手册·OOP 规约）。

use super::{Ctx, is_floating_literal, seq};
use crate::lex::Kind;
use openidea_plugin::{Diagnostic, Severity};

/// 手册·OOP 规约：用常量或确定有值的对象调用 equals，避免空指针。
pub fn equals_literal_first(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["."]) {
            continue;
        }
        let Some(m) = ctx.code.get(i + 1) else { continue };
        if !matches!(ctx.slice(m), "equals" | "equalsIgnoreCase") || !seq(ctx, i + 2, &["("]) {
            continue;
        }
        // 第一个实参是字面量，而调用方不是，才需要交换。
        let Some(arg) = ctx.next_token(ctx.code[i + 2].end) else { continue };
        if !matches!(arg.kind, Kind::String | Kind::Char) {
            continue;
        }
        let dot = ctx.code[i].start;
        if ctx.prev_token(dot).is_some_and(|t| matches!(t.kind, Kind::String | Kind::Char)) {
            continue;
        }
        out.push(ctx.diag(
            m.start,
            m.end,
            Severity::Warning,
            ctx.t(
                "equals 应由常量或确定有值的对象来调用：写成 \"test\".equals(obj) 而不是 obj.equals(\"test\")，\
                 否则 obj 为 null 时会抛 NullPointerException（手册·OOP 规约）",
                "Call equals on a constant or a known non-null object: write \"test\".equals(obj) instead of \
                 obj.equals(\"test\"), which throws NullPointerException when obj is null (Handbook: OOP)",
            ),
        ));
    }
    out
}

/// 手册·OOP 规约：避免用 double/float 构造 BigDecimal，精度不可预测；
/// 改用 new BigDecimal("0.1") 或 BigDecimal.valueOf(0.1)。只在实参是浮点字面量时判定，
/// 变量类型无法解析，保守放过。
pub fn bigdecimal_double(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["new"]) {
            continue;
        }
        if !ctx.code.get(i + 1).is_some_and(|t| t.kind == Kind::Ident) {
            continue;
        }
        let mut k = i + 1;
        while seq(ctx, k + 1, &["."]) && ctx.code.get(k + 2).is_some_and(|t| t.kind == Kind::Ident) {
            k += 2;
        }
        if ctx.slice(&ctx.code[k]) != "BigDecimal" || !seq(ctx, k + 1, &["("]) {
            continue;
        }
        let Some(arg) = ctx.code.get(k + 2) else { continue };
        if arg.kind != Kind::Number || !is_floating_literal(ctx.slice(arg)) {
            continue;
        }
        out.push(ctx.diag(
            arg.start,
            arg.end,
            Severity::Warning,
            ctx.t(
                "避免用 double/float 构造 BigDecimal：new BigDecimal(0.1) 实际得到 \
                 0.1000000000000000055511151231257827…，精度不可预测；请改用 new BigDecimal(\"0.1\") \
                 或 BigDecimal.valueOf(0.1)（手册·OOP 规约）",
                "Do not construct BigDecimal from a double/float: new BigDecimal(0.1) actually yields \
                 0.1000000000000000055511151231257827…, an unpredictable precision error. Use new BigDecimal(\"0.1\") \
                 or BigDecimal.valueOf(0.1) instead (Handbook: OOP)",
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn count(src: &str) -> usize {
        let _mock = MockHost::install();
        equals_literal_first(&Ctx::new(src)).len()
    }

    #[test]
    fn a_variable_receiver_with_a_literal_argument_is_flagged() {
        assert_eq!(count("class A { void f() { obj.equals(\"test\"); } }"), 1);
        assert_eq!(count("class A { void f() { getObj().equalsIgnoreCase(\"x\"); } }"), 1);
        assert_eq!(count("class A { void f() { obj.equals('c'); } }"), 1);
    }

    #[test]
    fn the_literal_first_form_is_already_correct() {
        assert_eq!(count("class A { void f() { \"test\".equals(obj); } }"), 0);
        assert_eq!(count("class A { void f() { \"test\".equalsIgnoreCase(obj); } }"), 0);
        assert_eq!(count("class A { void f() { CONST.equals(obj); } }"), 0);
    }

    #[test]
    fn two_variables_and_other_methods_are_left_alone() {
        assert_eq!(count("class A { void f() { a.equals(b); } }"), 0);
        assert_eq!(count("class A { void f() { obj.hashCode(); } }"), 0);
        assert_eq!(count("class A { void f() { obj.equals(); } }"), 0);
        assert_eq!(count("class A { String s = \"obj.equals(\\\"x\\\")\"; }"), 0);
    }

    #[test]
    fn the_range_covers_the_method_name() {
        let _mock = MockHost::install();
        let ctx = Ctx::new("obj.equals(\"x\");");
        let got = equals_literal_first(&ctx);
        assert_eq!(got[0].range.start.character, 4);
        assert_eq!(got[0].range.end.character, 10);
    }

    fn bigdec(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        bigdecimal_double(&Ctx::new(src))
    }

    #[test]
    fn flags_the_floating_point_constructor() {
        assert_eq!(bigdec("BigDecimal a = new BigDecimal(0.1);").len(), 1);
        assert_eq!(bigdec("BigDecimal a = new BigDecimal(0.1f);").len(), 1);
        assert_eq!(bigdec("BigDecimal a = new BigDecimal(1e-3);").len(), 1);
        assert_eq!(bigdec("BigDecimal a = new java.math.BigDecimal(0.1);").len(), 1);
        assert_eq!(bigdec("BigDecimal a = new BigDecimal(0.1, MathContext.DECIMAL64);").len(), 1);
    }

    #[test]
    fn string_valueof_and_integer_forms_are_fine() {
        assert_eq!(bigdec("BigDecimal a = new BigDecimal(\"0.1\");").len(), 0);
        assert_eq!(bigdec("BigDecimal a = BigDecimal.valueOf(0.1);").len(), 0);
        assert_eq!(bigdec("BigDecimal a = new BigDecimal(1);").len(), 0);
        assert_eq!(bigdec("BigDecimal a = new BigDecimal(1L);").len(), 0);
        assert_eq!(bigdec("BigDecimal a = new BigDecimal(x);").len(), 0);
    }

    #[test]
    fn bigdecimal_literals_in_strings_and_comments_are_ignored() {
        assert_eq!(bigdec("String s = \"new BigDecimal(0.1)\"; // new BigDecimal(0.2)").len(), 0);
    }

    #[test]
    fn the_bigdecimal_range_covers_the_literal() {
        let src = "BigDecimal a = new BigDecimal(0.1);";
        let d = &bigdec(src)[0];
        assert_eq!(&src[d.range.start.character as usize..d.range.end.character as usize], "0.1");
    }
}
