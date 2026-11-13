//! 其他

use super::{Ctx, is_floating_literal, seq};
use crate::lex::Kind;
use openidea_plugin::{Diagnostic, Severity};

/// SimpleDateFormat / DateTimeFormatter pattern letters, plus common separators.
const PATTERN_CHARS: &str = "GyYMLwWDdFEuaHkKhmsSzZX";
const APACHE_BEANUTILS: [&str; 8] = ["org", ".", "apache", ".", "commons", ".", "beanutils", "."];

fn is_date_pattern(s: &str) -> bool {
    s.chars().all(|c| PATTERN_CHARS.contains(c) || matches!(c, '-' | '/' | ':' | '.' | ' ' | ',' | '\''))
}

/// 手册·其他 6：YYYY 是 week-based year，跨年那一周会返回下一年。
pub fn date_pattern_year(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for t in ctx.of_kind(Kind::String) {
        let raw = ctx.slice(t);
        let Some(content) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
            continue;
        };
        if content.len() > 64 || !content.contains("YYYY") || !is_date_pattern(content) {
            continue;
        }
        let base = t.start + 1;
        for (k, _) in content.match_indices("YYYY") {
            out.push(ctx.diag(
                base + k,
                base + k + 4,
                Severity::Error,
                ctx.t(
                    "日期格式使用了 YYYY（week-based year），跨年那一周会返回下一年，应改为 yyyy；\
                     同时注意大写 M 是月、小写 m 是分，大写 H 是 24 小时制、小写 h 是 12 小时制（手册·其他 6）",
                    "The date pattern uses YYYY (week-based year), which returns the next year in the week that \
                     spans New Year; use yyyy instead. Note also that uppercase M is month while lowercase m is \
                     minute, and uppercase H is 24-hour while lowercase h is 12-hour (Handbook: Misc 6)",
                ),
            ));
        }
    }
    out
}

/// 手册·其他 5：取当前毫秒数用 System.currentTimeMillis()，不要 new Date().getTime()。
pub fn current_time_millis(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["new"]) {
            continue;
        }
        let j = if seq(ctx, i + 1, &["java", ".", "util", ".", "Date"]) {
            i + 6
        } else if seq(ctx, i + 1, &["Date"]) {
            i + 2
        } else {
            continue;
        };
        if !seq(ctx, j, &["(", ")", ".", "getTime", "(", ")"]) {
            continue;
        }
        out.push(ctx.diag(
            ctx.code[i].start,
            ctx.code[j + 5].end,
            Severity::Warning,
            ctx.t(
                "获取当前毫秒数应使用 System.currentTimeMillis()，new Date().getTime() 会多创建一个 \
                 Date 对象；需要更高精度用 System.nanoTime()（手册·其他 5）",
                "Use System.currentTimeMillis() for the current time in milliseconds: new Date().getTime() \
                 allocates an extra Date object. For higher precision use System.nanoTime() (Handbook: Misc 5)",
            ),
        ));
    }
    out
}

/// 手册·其他 1：避免用 Apache BeanUtils 复制属性。Spring 的同名工具不在禁止之列，
/// 所以必须先确认 import 或全限定名来自 org.apache.commons.beanutils。
pub fn apache_beanutils(ctx: &Ctx) -> Vec<Diagnostic> {
    let imported = (0..ctx.code.len()).any(|i| {
        seq(ctx, i, &["import", "org", ".", "apache", ".", "commons", ".", "beanutils", "."])
            && ctx.code.get(i + 9).is_some_and(|t| matches!(ctx.slice(t), "BeanUtils" | "*"))
    });
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["BeanUtils", ".", "copyProperties", "("]) {
            continue;
        }
        let qualified = i >= APACHE_BEANUTILS.len() && seq(ctx, i - APACHE_BEANUTILS.len(), &APACHE_BEANUTILS);
        if !imported && !qualified {
            continue;
        }
        let t = &ctx.code[i + 2];
        out.push(ctx.diag(
            t.start,
            t.end,
            Severity::Warning,
            ctx.t(
                "避免使用 Apache BeanUtils 复制属性，其性能较差；可改用 Spring BeanUtils 或显式赋值（手册·其他 1）",
                "Avoid copying properties with Apache BeanUtils, which performs poorly; use Spring BeanUtils or \
                 explicit assignment instead (Handbook: Misc 1)",
            ),
        ));
    }
    out
}

/// 手册·其它：正则要预编译复用，不要在方法体内反复 Pattern.compile。
pub fn pattern_compile_in_method(ctx: &Ctx) -> Vec<Diagnostic> {
    let bodies: Vec<(usize, usize)> = ctx.decl().methods.iter().filter_map(|m| m.body).collect();
    if bodies.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["Pattern", ".", "compile", "("]) {
            continue;
        }
        if !bodies.iter().any(|&(s, e)| i >= s && i < e) {
            continue;
        }
        out.push(ctx.diag(
            ctx.code[i].start,
            ctx.code[i + 2].end,
            Severity::Warning,
            ctx.t(
                "不要在方法体内调用 Pattern.compile：每次调用都会重新编译正则，\
                 应提取为 static final Pattern 常量复用（手册·其它）",
                "Do not call Pattern.compile inside a method body: every call recompiles the regex. Extract it \
                 into a static final Pattern constant and reuse it (Handbook: Misc)",
            ),
        ));
    }
    out
}

/// 手册·其它：取整数随机数用 Random.nextInt/nextLong，不要把 Math.random() 强转成 int/long。
pub fn math_random_cast(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["("])
            || !ctx.code.get(i + 1).is_some_and(|t| matches!(ctx.slice(t), "int" | "long"))
            || !seq(ctx, i + 2, &[")"])
        {
            continue;
        }
        // 强转后可能直接跟 Math.random()，也可能再包一层括号：`(long) (Math.random() * 10)`。
        let mut k = i + 3;
        if seq(ctx, k, &["("]) {
            k += 1;
        }
        if !seq(ctx, k, &["Math", ".", "random", "("]) {
            continue;
        }
        let Some(close) = ctx.parens()[k + 3] else { continue };
        out.push(ctx.diag(
            ctx.code[i].start,
            ctx.code[close].end,
            Severity::Warning,
            ctx.t(
                "取某个范围内的整数随机数应使用 Random.nextInt(n) / nextLong()，把 Math.random() 强转成 \
                 int/long 容易写错边界、性能也差；Math.random() 返回 [0,1) 的 double（手册·其它）",
                "Use Random.nextInt(n) / nextLong() for integer random values in a range: casting Math.random() \
                 to int/long is error-prone at the boundaries and slower. Math.random() returns a double in [0,1) \
                 (Handbook: Misc)",
            ),
        ));
    }
    out
}

/// 手册·其它：浮点数之间不用 == / != 判等。变量类型无法解析，只在某一侧是浮点字面量时判定。
pub fn float_equality(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !matches!(ctx.slice(&ctx.code[i]), "==" | "!=") {
            continue;
        }
        let prev_float =
            i > 0 && ctx.code[i - 1].kind == Kind::Number && is_floating_literal(ctx.slice(&ctx.code[i - 1]));
        let next_float =
            ctx.code.get(i + 1).is_some_and(|t| t.kind == Kind::Number && is_floating_literal(ctx.slice(t)));
        if !prev_float && !next_float {
            continue;
        }
        out.push(ctx.diag(
            ctx.code[i].start,
            ctx.code[i].end,
            Severity::Warning,
            ctx.t(
                "浮点数之间不要用 == 或 != 判等：二进制无法精确表示大多数十进制小数，\
                 应改用误差范围（Math.abs(a - b) < 1e-6）或 BigDecimal（手册·其它）",
                "Do not compare floating-point values with == or !=: binary cannot exactly represent most decimal \
                 fractions. Use an epsilon range (Math.abs(a - b) < 1e-6) or BigDecimal instead (Handbook: Misc)",
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn year(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        date_pattern_year(&Ctx::new(src))
    }

    fn millis(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        current_time_millis(&Ctx::new(src))
    }

    fn beanutils(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        apache_beanutils(&Ctx::new(src))
    }

    #[test]
    fn flags_week_based_year_in_a_pattern() {
        assert_eq!(year(r#"new SimpleDateFormat("YYYY-MM-dd");"#).len(), 1);
        assert_eq!(year(r#"DateTimeFormatter.ofPattern("YYYY/MM/dd HH:mm:ss");"#).len(), 1);
        assert_eq!(year(r#"String p = "YYYY";"#).len(), 1);
        assert_eq!(year(r#"new SimpleDateFormat("yyyy-MM-dd");"#).len(), 0);
    }

    #[test]
    fn the_range_covers_the_yyyy_inside_the_quotes() {
        let src = r#"new SimpleDateFormat("YYYY-MM-dd");"#;
        let d = &year(src)[0];
        assert_eq!(&src[d.range.start.character as usize..d.range.end.character as usize], "YYYY");
        assert_eq!(d.severity, Severity::Error);
    }

    #[test]
    fn prose_and_comments_are_not_patterns() {
        assert_eq!(year(r#"String s = "HAPPY NEW YEAR YYYY";"#).len(), 0);
        assert_eq!(year(r#"// "YYYY-MM-dd""#).len(), 0);
        assert_eq!(year(r#"String s = "YYYY-MM-dd\"";"#).len(), 0);
        assert_eq!(year("String s = \"\"\"\nYYYY-MM-dd\n\"\"\";").len(), 0);
    }

    #[test]
    fn every_occurrence_in_one_pattern_is_reported() {
        assert_eq!(year(r#"String p = "YYYY-YYYY";"#).len(), 2);
    }

    #[test]
    fn flags_new_date_get_time_in_both_spellings() {
        assert_eq!(millis("long t = new Date().getTime();").len(), 1);
        assert_eq!(millis("long t = new java.util.Date().getTime();").len(), 1);
        assert_eq!(millis("long t = System.currentTimeMillis();").len(), 0);
    }

    #[test]
    fn other_date_calls_are_left_alone() {
        assert_eq!(millis("long t = date.getTime();").len(), 0);
        assert_eq!(millis("int y = new Date().getYear();").len(), 0);
        assert_eq!(millis("long t = new Date(0).getTime();").len(), 0);
        assert_eq!(millis(r#"String s = "new Date().getTime()";"#).len(), 0);
    }

    #[test]
    fn the_range_covers_the_whole_expression() {
        let src = "long t = new Date().getTime();";
        let d = &millis(src)[0];
        assert_eq!(&src[d.range.start.character as usize..d.range.end.character as usize], "new Date().getTime()");
    }

    #[test]
    fn apache_beanutils_needs_the_apache_import() {
        let call = "BeanUtils.copyProperties(dst, src);";
        assert_eq!(beanutils(&format!("import org.apache.commons.beanutils.BeanUtils;\n{call}")).len(), 1);
        assert_eq!(beanutils(&format!("import org.apache.commons.beanutils.*;\n{call}")).len(), 1);
        assert_eq!(beanutils(&format!("import org.springframework.beans.BeanUtils;\n{call}")).len(), 0);
        assert_eq!(beanutils(call).len(), 0);
    }

    #[test]
    fn a_qualified_apache_call_needs_no_import() {
        assert_eq!(beanutils("org.apache.commons.beanutils.BeanUtils.copyProperties(a, b);").len(), 1);
        assert_eq!(beanutils("org.springframework.beans.BeanUtils.copyProperties(a, b);").len(), 0);
    }

    #[test]
    fn strings_and_comments_do_not_count_as_imports() {
        let src = r#"// import org.apache.commons.beanutils.BeanUtils;
String s = "import org.apache.commons.beanutils.BeanUtils;";
BeanUtils.copyProperties(a, b);"#;
        assert_eq!(beanutils(src).len(), 0);
    }

    fn compiles(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        pattern_compile_in_method(&Ctx::new(src))
    }

    #[test]
    fn compiling_inside_a_method_body_is_flagged() {
        assert_eq!(compiles("class A { void f() { Pattern p = Pattern.compile(\"a+\"); } }").len(), 1);
        assert_eq!(compiles("class A { void f() { if (a) { Pattern.compile(\"x\"); } } }").len(), 1);
        assert_eq!(
            compiles("class A { void f() { Pattern.compile(\"a\"); } void g() { Pattern.compile(\"b\"); } }").len(),
            2
        );
    }

    #[test]
    fn a_precompiled_constant_is_the_recommended_form() {
        assert_eq!(compiles("class A { static final Pattern P = Pattern.compile(\"a+\"); }").len(), 0);
        assert_eq!(compiles("class A { private Pattern p = Pattern.compile(\"a+\"); }").len(), 0);
        assert_eq!(compiles("class A { void f(); }").len(), 0);
    }

    #[test]
    fn literals_comments_and_other_calls_are_ignored() {
        assert_eq!(compiles("class A { String s = \"Pattern.compile(\\\"a\\\")\"; }").len(), 0);
        assert_eq!(compiles("class A { void f() { // Pattern.compile(\"a\")\n g(); } }").len(), 0);
        assert_eq!(compiles("class A { void f() { matcher.compile(\"a\"); } }").len(), 0);
    }

    fn math_random(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        math_random_cast(&Ctx::new(src))
    }

    #[test]
    fn flags_int_and_long_casts_of_math_random() {
        assert_eq!(math_random("int r = (int) Math.random();").len(), 1);
        assert_eq!(math_random("long r = (long) (Math.random() * 10);").len(), 1);
        assert_eq!(math_random("int r = (int) (Math.random() * 100);").len(), 1);
    }

    #[test]
    fn proper_random_and_plain_math_random_are_fine() {
        assert_eq!(math_random("int r = new Random().nextInt(10);").len(), 0);
        assert_eq!(math_random("double r = Math.random();").len(), 0);
        assert_eq!(math_random("int r = (int) someDouble;").len(), 0);
        assert_eq!(math_random("int r = (int) (a + Math.random());").len(), 0);
        assert_eq!(math_random("String s = \"(int) Math.random()\";").len(), 0);
    }

    fn float_eq(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        float_equality(&Ctx::new(src))
    }

    #[test]
    fn flags_equality_against_a_floating_literal() {
        assert_eq!(float_eq("if (x == 0.1) {}").len(), 1);
        assert_eq!(float_eq("if (0.7f != y) {}").len(), 1);
        assert_eq!(float_eq("if (x == 1e-6) {}").len(), 1);
        assert_eq!(float_eq("if (0.0 == x) {}").len(), 1);
    }

    #[test]
    fn integer_equality_and_epsilon_ranges_are_fine() {
        assert_eq!(float_eq("if (a == b) {}").len(), 0);
        assert_eq!(float_eq("if (n == 1) {}").len(), 0);
        assert_eq!(float_eq("if (Math.abs(a - b) < 1e-6) {}").len(), 0);
        assert_eq!(float_eq("String s = \"x == 0.1\";").len(), 0);
    }
}
