//! 命名风格（手册·命名风格 1—9）与方法规模。

use super::Ctx;
use super::annotation_name;
use super::decl::{Span, TypeKind, has_modifier};
use openidea_plugin::{Diagnostic, Severity};

/// 每个声明出来的名字，附带消息里用的名词。枚举常量只参与符号与中文两条规则。
fn declared(ctx: &Ctx) -> Vec<(Span, &'static str)> {
    let d = ctx.decl();
    let mut out = Vec::new();
    for t in &d.types {
        out.push((t.name, ctx.t("类名", "Class name")));
    }
    for m in &d.methods {
        if !m.constructor {
            out.push((m.name, ctx.t("方法名", "Method name")));
        }
        for p in &m.params {
            out.push((p.name, ctx.t("参数名", "Parameter name")));
        }
    }
    for f in &d.fields {
        out.push((f.name, ctx.t("成员变量名", "Field name")));
    }
    for l in &d.locals {
        out.push((l.name, ctx.t("局部变量名", "Local variable name")));
    }
    for c in &d.enum_constants {
        out.push((c.name, ctx.t("枚举常量名", "Enum constant name")));
    }
    out
}

fn edged(name: &str) -> bool {
    matches!(name.chars().next(), Some('_' | '$')) || matches!(name.chars().next_back(), Some('_' | '$'))
}

/// 手册·命名风格 1：命名不能以下划线或美元符号开始或结束。
pub fn no_edge_symbol(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (span, noun) in declared(ctx) {
        if edged(span.text(ctx)) {
            let message = if ctx.zh() {
                format!("{noun}不能以下划线或美元符号开始，也不能以下划线或美元符号结束（手册·命名风格 1）")
            } else {
                format!("{noun} must not start or end with an underscore or a dollar sign (Handbook: Naming Style 1)")
            };
            out.push(ctx.diag(span.start, span.end, Severity::Warning, &message));
        }
    }
    out
}

/// 手册·命名风格 2：不允许直接使用中文。拼音与英文混合无法可靠判定，故不检查。
pub fn no_chinese(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (span, noun) in declared(ctx) {
        if span.text(ctx).chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
            let message = if ctx.zh() {
                format!("{noun}严禁使用中文，也不要用拼音与英文混合（手册·命名风格 2）")
            } else {
                format!(
                    "{noun} must not contain Chinese characters, and must not mix pinyin with English \
                     (Handbook: Naming Style 2)"
                )
            };
            out.push(ctx.diag(span.start, span.end, Severity::Warning, &message));
        }
    }
    out
}

/// 领域模型后缀允许连续大写：UserDO、PageVO。
const DOMAIN_SUFFIXES: [&str; 7] = ["DTO", "UID", "DO", "BO", "VO", "AO", "PO"];

fn adjacent_upper(s: &str) -> bool {
    s.as_bytes().windows(2).any(|w| w[0].is_ascii_uppercase() && w[1].is_ascii_uppercase())
}

fn upper_camel(name: &str) -> bool {
    if !name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) || !name.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return false;
    }
    // 领域后缀写成混合大小写同样是反例：UserDo 应为 UserDO。
    // 词干不足 3 个字符的（ToDo）是普通单词，不当作领域模型。
    for s in DOMAIN_SUFFIXES {
        if name.len() < s.len() + 3 {
            continue;
        }
        let tail = &name[name.len() - s.len()..];
        if tail != s && tail.eq_ignore_ascii_case(s) && tail.starts_with(|c: char| c.is_ascii_uppercase()) {
            return false;
        }
    }
    let stem = DOMAIN_SUFFIXES
        .iter()
        .find(|s| name.len() >= s.len() && name.ends_with(*s))
        .map_or(name, |s| &name[..name.len() - s.len()]);
    !adjacent_upper(stem)
}

/// 手册·命名风格 3：类名 UpperCamelCase，DO/BO/DTO/VO/AO/PO/UID 等除外。
pub fn class_upper_camel(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for t in &ctx.decl().types {
        let name = t.name.text(ctx);
        // 下划线与美元符号由命名风格 1 负责，避免同一处重复报。
        if edged(name) || upper_camel(name) {
            continue;
        }
        out.push(ctx.diag(
            t.name.start,
            t.name.end,
            Severity::Warning,
            ctx.t(
                "类名应使用 UpperCamelCase 风格，如 MarcoPolo、UserDO、XmlService；\
                 不要写成 macroPolo、UserDo、XMLService（手册·命名风格 3）",
                "Class names should use UpperCamelCase, e.g. MarcoPolo, UserDO, XmlService; \
                 not macroPolo, UserDo or XMLService (Handbook: Naming Style 3)",
            ),
        ));
    }
    out
}

fn lower_camel(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_alphanumeric())
}

fn is_constant(ctx: &Ctx, f: &super::decl::FieldDecl) -> bool {
    f.in_interface || (has_modifier(ctx, &f.modifiers, "static") && has_modifier(ctx, &f.modifiers, "final"))
}

/// 手册·命名风格 4：方法名、参数名、成员变量、局部变量统一 lowerCamelCase。
pub fn lower_camel_names(ctx: &Ctx) -> Vec<Diagnostic> {
    let d = ctx.decl();
    let mut targets: Vec<(Span, &str)> = Vec::new();
    for m in &d.methods {
        if !m.constructor {
            targets.push((m.name, ctx.t("方法名", "Method name")));
        }
        for p in &m.params {
            targets.push((p.name, ctx.t("参数名", "Parameter name")));
        }
    }
    for f in &d.fields {
        if !is_constant(ctx, f) {
            targets.push((f.name, ctx.t("成员变量名", "Field name")));
        }
    }
    for l in &d.locals {
        targets.push((l.name, ctx.t("局部变量名", "Local variable name")));
    }

    let mut out = Vec::new();
    for (span, noun) in targets {
        let name = span.text(ctx);
        if edged(name) || lower_camel(name) {
            continue;
        }
        let message = if ctx.zh() {
            format!("{noun}应使用 lowerCamelCase 风格，如 localValue、getHttpMessage（手册·命名风格 4）")
        } else {
            format!("{noun} should use lowerCamelCase, e.g. localValue, getHttpMessage (Handbook: Naming Style 4)")
        };
        out.push(ctx.diag(span.start, span.end, Severity::Warning, &message));
    }
    out
}

fn upper_snake(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name.split('_').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// 日志句柄不是常量，按惯例小写；serialVersionUID 由 JVM 规定，不能改名。
fn exempt_constant(name: &str, ty: &str) -> bool {
    name == "serialVersionUID" || ty.ends_with("Logger") || ty.ends_with("Log")
}

/// 手册·命名风格 5：常量命名全部大写，单词间用下划线隔开。
pub fn constant_upper_snake(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for f in &ctx.decl().fields {
        if !is_constant(ctx, f) {
            continue;
        }
        let name = f.name.text(ctx);
        if exempt_constant(name, &f.ty) || upper_snake(name) {
            continue;
        }
        out.push(ctx.diag(
            f.name.start,
            f.name.end,
            Severity::Warning,
            ctx.t(
                "常量命名应全部大写，单词间用下划线隔开，如 MAX_STOCK_COUNT（手册·命名风格 5）",
                "Constant names should be all uppercase with words separated by underscores, \
                 e.g. MAX_STOCK_COUNT (Handbook: Naming Style 5)",
            ),
        ));
    }
    out
}

fn exception_type(name: &str) -> bool {
    name == "Throwable" || name.ends_with("Exception") || name.ends_with("Error")
}

/// 手册·命名风格 6：抽象类以 Abstract 或 Base 开头，异常类以 Exception 结尾。
pub fn abstract_and_exception_names(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for t in &ctx.decl().types {
        if t.kind != TypeKind::Class {
            continue;
        }
        let name = t.name.text(ctx);
        if has_modifier(ctx, &t.modifiers, "abstract") && !(name.starts_with("Abstract") || name.starts_with("Base")) {
            out.push(ctx.diag(
                t.name.start,
                t.name.end,
                Severity::Warning,
                ctx.t(
                    "抽象类名应以 Abstract 或 Base 开头（手册·命名风格 6）",
                    "Abstract class names should start with Abstract or Base (Handbook: Naming Style 6)",
                ),
            ));
        }
        if t.superclass.as_deref().is_some_and(exception_type) && !name.ends_with("Exception") {
            out.push(ctx.diag(
                t.name.start,
                t.name.end,
                Severity::Warning,
                ctx.t(
                    "异常类名应以 Exception 结尾（手册·命名风格 6）",
                    "Exception class names should end with Exception (Handbook: Naming Style 6)",
                ),
            ));
        }
    }
    out
}

/// 手册·命名风格 7：类型与中括号紧挨相连来表示数组，`int[] args` 而非 `int args[]`。
pub fn array_brackets(ctx: &Ctx) -> Vec<Diagnostic> {
    let d = ctx.decl();
    let mut targets: Vec<(Span, &str)> = Vec::new();
    for f in &d.fields {
        if f.array_after_name {
            targets.push((f.name, ctx.t("成员变量", "field")));
        }
    }
    for m in &d.methods {
        for p in &m.params {
            if p.array_after_name {
                targets.push((p.name, ctx.t("参数", "parameter")));
            }
        }
    }
    for l in &d.locals {
        if l.array_after_name {
            targets.push((l.name, ctx.t("局部变量", "local variable")));
        }
    }
    targets
        .into_iter()
        .map(|(span, noun)| {
            let name = span.text(ctx);
            let message = if ctx.zh() {
                format!("{noun}的数组中括号应紧跟类型，写成 String[] {name} 而不是 String {name}[]（手册·命名风格 7）")
            } else {
                format!(
                    "Array brackets on a {noun} belong to the type: write String[] {name}, not String {name}[] \
                     (Handbook: Naming Style 7)"
                )
            };
            ctx.diag(span.start, span.end, Severity::Warning, &message)
        })
        .collect()
}

/// 手册·命名风格 8：POJO 中布尔类型变量不要加 is 前缀，部分框架解析会引起序列化错误。
pub fn boolean_no_is_prefix(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for f in &ctx.decl().fields {
        if !matches!(f.ty.as_str(), "boolean" | "Boolean" | "java.lang.Boolean") {
            continue;
        }
        let name = f.name.text(ctx);
        if name.len() > 2 && name.starts_with("is") && name.as_bytes()[2].is_ascii_uppercase() {
            out.push(ctx.diag(
                f.name.start,
                f.name.end,
                Severity::Warning,
                ctx.t(
                    "布尔类型变量不要加 is 前缀，部分框架反向解析时会误认为字段名少了 is，\
                     导致序列化错误；用 deleted 而不是 isDeleted（手册·命名风格 8）",
                    "Do not prefix boolean variables with is: some frameworks parse the name back without the is, \
                     which breaks serialization. Use deleted instead of isDeleted (Handbook: Naming Style 8)",
                ),
            ));
        }
    }
    out
}

/// 手册·命名风格 9：包名统一使用小写。
pub fn package_lowercase(ctx: &Ctx) -> Vec<Diagnostic> {
    let Some(p) = ctx.decl().package else {
        return Vec::new();
    };
    if !p.text(ctx).chars().any(|c| c.is_ascii_uppercase()) {
        return Vec::new();
    }
    vec![ctx.diag(
        p.start,
        p.end,
        Severity::Warning,
        ctx.t(
            "包名应统一使用小写，点分隔符之间有且仅有一个自然语义的英语单词，如 com.alibaba.effles.util（手册·命名风格 9）",
            "Package names must be all lowercase, with exactly one natural English word between the dots, \
             e.g. com.alibaba.effles.util (Handbook: Naming Style 9)",
        ),
    )]
}

/// 手册·其它：单个方法的总行数不超过设置的上限（默认 80 行），整行注释不计入。
pub fn method_too_long(ctx: &Ctx) -> Vec<Diagnostic> {
    let max_lines = ctx.settings().max_method_lines;
    let d = ctx.decl();
    if d.methods.is_empty() {
        return Vec::new();
    }
    // 含代码的行号，升序去重；区间行数用二分统计，避免每个方法都扫一遍 token。
    let mut code_lines: Vec<u32> = Vec::new();
    for t in &ctx.code {
        let line = ctx.line(t.start);
        if code_lines.last() != Some(&line) {
            code_lines.push(line);
        }
    }

    let mut out = Vec::new();
    for m in &d.methods {
        let Some((_, close)) = m.body else { continue };
        let first = m.modifiers.first().map_or(m.name.start, |s| s.start);
        let start = ctx.line(first);
        let end = ctx.line(ctx.code[close].start);
        let lines = code_lines.partition_point(|&l| l <= end) - code_lines.partition_point(|&l| l < start);
        if lines as u32 > max_lines {
            let message = if ctx.zh() {
                format!(
                    "单个方法的总行数不应超过 {max_lines} 行，当前 {lines} 行（不含整行注释）；\
                     请把可复用的逻辑抽成独立方法（手册·其它）"
                )
            } else {
                format!(
                    "A method should not exceed {max_lines} lines in total; this one has {lines} (whole-line comments \
                     excluded). Extract reusable logic into its own method (Handbook: Misc)"
                )
            };
            out.push(ctx.diag(m.name.start, m.name.end, Severity::Warning, &message));
        }
    }
    out
}

/// 手册·命名风格：测试类名应以 Test 结尾。含 @Test（及 @ParameterizedTest 等）方法的类视为测试类。
pub fn test_class_suffix(ctx: &Ctx) -> Vec<Diagnostic> {
    let d = ctx.decl();
    let tests: Vec<usize> = (0..ctx.code.len())
        .filter(|&i| ctx.slice(&ctx.code[i]) == "@")
        .filter_map(|i| annotation_name(ctx, i).map(|(name, _)| (i, name)))
        .filter(|(_, name)| name.ends_with("Test"))
        .map(|(i, _)| i)
        .collect();
    if tests.is_empty() {
        return Vec::new();
    }
    // 每个 @Test 归属最内层的类型，避免连带把外层类也报出来。
    let mut flagged = vec![false; d.types.len()];
    for &a in &tests {
        let innermost = d
            .types
            .iter()
            .enumerate()
            .filter(|(_, t)| t.body.is_some_and(|(s, e)| s <= a && a < e))
            .min_by_key(|(_, t)| t.body.map_or(usize::MAX, |(s, e)| e - s))
            .map(|(idx, _)| idx);
        if let Some(idx) = innermost {
            flagged[idx] = true;
        }
    }
    let mut out = Vec::new();
    for (idx, t) in d.types.iter().enumerate() {
        if !flagged[idx] || t.name.text(ctx).ends_with("Test") {
            continue;
        }
        out.push(ctx.diag(
            t.name.start,
            t.name.end,
            Severity::Warning,
            ctx.t(
                "测试类名应以 Test 结尾，如 DemoTest；含 @Test 方法的类被视为测试类（手册·命名风格）",
                "Test class names should end with Test, e.g. DemoTest; a class with @Test methods is treated as a \
                 test class (Handbook: Naming Style)",
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn messages(rule: fn(&Ctx) -> Vec<Diagnostic>, src: &str) -> Vec<String> {
        let _mock = MockHost::install();
        rule(&Ctx::new(src)).into_iter().map(|d| d.message).collect()
    }

    fn spans(rule: fn(&Ctx) -> Vec<Diagnostic>, src: &str) -> Vec<String> {
        let _mock = MockHost::install();
        let ctx = Ctx::new(src);
        rule(&ctx)
            .into_iter()
            .map(|d| {
                let r = d.range;
                format!("{}:{}-{}", r.start.line, r.start.character, r.end.character)
            })
            .collect()
    }

    #[test]
    fn nested_types_with_modifiers_are_not_fields() {
        let src = "class Outer {\n    private static class Inner {\n        private int BadName = 0;\n    }\n\
                   \x20   public enum Mode { FAST }\n    protected interface Api { void DoIt(); }\n}\n";
        let mut got = spans(lower_camel_names, src);
        got.sort();
        assert_eq!(got, ["2:20-27", "5:35-39"], "{got:?}");
    }

    #[test]
    fn constructors_keep_the_class_name_but_their_params_are_checked() {
        let got = messages(lower_camel_names, "class OrderService { OrderService(int BadParam) {} void GetIt() {} }");
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got.iter().any(|m| m.starts_with("Parameter name")), "{got:?}");
        assert!(got.iter().any(|m| m.starts_with("Method name")), "{got:?}");
        assert!(messages(lower_camel_names, "enum Mode { FAST; Mode(int goodParam) {} }").is_empty());
    }

    #[test]
    fn a_constructor_matches_the_innermost_type_only() {
        let src = "class Outer { Outer() {} static class Inner { Inner() {} void Outer() {} } }";
        let got = messages(lower_camel_names, src);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].starts_with("Method name"), "{got:?}");
    }

    #[test]
    fn interface_fields_are_constants_even_without_modifiers() {
        let src = r#"interface Api { String GOOD_TIP = "x"; String badTip = "y"; }"#;
        assert!(messages(lower_camel_names, src).is_empty());
        assert_eq!(messages(constant_upper_snake, src).len(), 1);
        // 接口里的嵌套类是普通类，它的字段不隐式 static final。
        let nested = "interface Api { class Inner { int BadName = 0; } }";
        assert_eq!(messages(lower_camel_names, nested).len(), 1);
    }

    #[test]
    fn edge_symbols_are_flagged_on_every_kind_of_name() {
        let src = "class _A { int _f; void m_(int $p) { int l_ = 0; } }";
        assert_eq!(messages(no_edge_symbol, src).len(), 5);
        assert!(messages(no_edge_symbol, "class A { int f; void m(int p) { int l = 0; } }").is_empty());
        assert_eq!(messages(no_edge_symbol, "enum E { _A, B }").len(), 1);
    }

    #[test]
    fn edge_symbols_do_not_double_report_with_camel_rules() {
        let src = "class A { int _f; void _m() {} }";
        assert_eq!(messages(no_edge_symbol, src).len(), 2);
        assert!(messages(lower_camel_names, src).is_empty());
        assert!(messages(class_upper_camel, "class _A {}").is_empty());
    }

    #[test]
    fn chinese_names_are_flagged_but_pinyin_is_not() {
        assert_eq!(messages(no_chinese, "class 用户 { int 数量; }").len(), 2);
        assert!(messages(no_chinese, "class User { int shuliang; }").is_empty());
        assert!(messages(no_chinese, "class A { String s = \"中文\"; } // 中文注释").is_empty());
    }

    #[test]
    fn upper_camel_allows_domain_suffixes_only() {
        assert!(messages(class_upper_camel, "class MarcoPolo {} class UserDO {} class PageVO {}").is_empty());
        assert!(messages(class_upper_camel, "class XmlService {} class TcpUdpDeal {}").is_empty());
        assert_eq!(messages(class_upper_camel, "class macroPolo {}").len(), 1);
        assert_eq!(messages(class_upper_camel, "class UserDo {} class PageDto {} class OrderVo {}").len(), 3);
        assert_eq!(messages(class_upper_camel, "class XMLService {} class TCPUDPDeal {}").len(), 2);
    }

    #[test]
    fn ordinary_words_ending_like_a_suffix_are_left_alone() {
        assert!(messages(class_upper_camel, "class ToDo {} class Pseudo {} class Kudo {} class Video {}").is_empty());
        assert!(messages(class_upper_camel, "class A {} class DO {} class DTO {}").is_empty());
    }

    #[test]
    fn upper_camel_covers_interfaces_enums_and_records() {
        assert_eq!(messages(class_upper_camel, "interface userDao {} enum color {} record point(int x) {}").len(), 3);
        assert!(messages(class_upper_camel, "interface UserDao {} enum Color {} record Point(int x) {}").is_empty());
    }

    #[test]
    fn lower_camel_covers_methods_params_fields_and_locals() {
        let src = "class A { int BadField; void GetHttp(int BadParam) { int BadLocal = 0; } }";
        assert_eq!(messages(lower_camel_names, src).len(), 4);
        assert!(
            messages(
                lower_camel_names,
                "class A { int goodField; void getHttp(int goodParam) { int goodLocal = 0; } }"
            )
            .is_empty()
        );
        assert_eq!(messages(lower_camel_names, "class A { void f() { int local_value = 0; } }").len(), 1);
    }

    #[test]
    fn constants_are_exempt_from_lower_camel() {
        assert!(messages(lower_camel_names, "class A { static final int MAX_COUNT = 1; }").is_empty());
        assert_eq!(messages(lower_camel_names, "class A { static int MAX_COUNT = 1; }").len(), 1);
        assert_eq!(messages(lower_camel_names, "enum E { A_B, C_D; void f() {} }").len(), 0);
    }

    #[test]
    fn constants_must_be_upper_snake() {
        assert!(
            messages(
                constant_upper_snake,
                "class A { static final int MAX_STOCK_COUNT = 1; static final int X2 = 2; }"
            )
            .is_empty()
        );
        assert_eq!(messages(constant_upper_snake, "class A { static final int maxCount = 1; }").len(), 1);
        assert_eq!(
            messages(constant_upper_snake, "class A { static final int MAX__COUNT = 1; static final int _X = 2; }")
                .len(),
            2
        );
        assert!(messages(constant_upper_snake, "class A { int maxCount = 1; }").is_empty());
    }

    #[test]
    fn serial_version_uid_and_loggers_are_exempt() {
        let src = "class A { static final long serialVersionUID = 1L; static final Logger log = null; \
                   static final org.slf4j.Logger LOG2 = null; }";
        assert!(messages(constant_upper_snake, src).is_empty());
        assert_eq!(messages(constant_upper_snake, "class A { static final String app_name = \"x\"; }").len(), 1);
    }

    #[test]
    fn abstract_classes_need_the_prefix() {
        assert!(
            messages(abstract_and_exception_names, "abstract class AbstractUser {} abstract class BaseUser {}")
                .is_empty()
        );
        assert_eq!(messages(abstract_and_exception_names, "abstract class User {}").len(), 1);
        assert!(messages(abstract_and_exception_names, "class User {}").is_empty());
        assert!(messages(abstract_and_exception_names, "abstract interface I {}").is_empty());
    }

    #[test]
    fn exception_classes_need_the_suffix() {
        assert!(messages(abstract_and_exception_names, "class UserException extends RuntimeException {}").is_empty());
        assert_eq!(messages(abstract_and_exception_names, "class MyError extends RuntimeException {}").len(), 1);
        assert_eq!(messages(abstract_and_exception_names, "class Bad extends Exception {}").len(), 1);
        assert!(messages(abstract_and_exception_names, "class Ok extends Base {}").is_empty());
    }

    #[test]
    fn array_brackets_belong_to_the_type() {
        assert_eq!(
            messages(array_brackets, "class A { String args[]; void f(int nums[]) { int local[] = null; } }").len(),
            3
        );
        assert!(
            messages(array_brackets, "class A { String[] args; void f(int[] nums) { int[] local = null; } }")
                .is_empty()
        );
        assert_eq!(spans(array_brackets, "class A {\n  String args[];\n}"), ["1:9-13"]);
    }

    #[test]
    fn boolean_fields_must_not_start_with_is() {
        assert_eq!(messages(boolean_no_is_prefix, "class A { Boolean isDeleted; boolean isSuccess; }").len(), 2);
        assert!(
            messages(boolean_no_is_prefix, "class A { Boolean deleted; boolean success; String isName; }").is_empty()
        );
        assert!(messages(boolean_no_is_prefix, "class A { boolean island; }").is_empty());
        assert!(messages(boolean_no_is_prefix, "class A { void f() { boolean isDeleted = true; } }").is_empty());
    }

    #[test]
    fn packages_must_be_lowercase() {
        assert!(messages(package_lowercase, "package com.alibaba.effles.util;").is_empty());
        assert_eq!(messages(package_lowercase, "package com.Alibaba.util;").len(), 1);
        assert!(messages(package_lowercase, "class A {}").is_empty());
        assert_eq!(spans(package_lowercase, "package com.Alibaba;"), ["0:8-19"]);
    }

    #[test]
    fn long_methods_are_measured_in_code_lines() {
        let body = "int x = 0;\n".repeat(81);
        let src = format!("class A {{ void f() {{\n{body}}} }}");
        assert_eq!(messages(method_too_long, &src).len(), 1);

        let short = format!("class A {{ void f() {{\n{}}} }}", "int x = 0;\n".repeat(78));
        assert!(messages(method_too_long, &short).is_empty());
    }

    #[test]
    fn comment_only_lines_do_not_count_towards_the_limit() {
        let mut src = String::from("class A { void f() {\n");
        for _ in 0..70 {
            src.push_str("// 说明\n");
        }
        for _ in 0..20 {
            src.push_str("int x = 0;\n");
        }
        src.push_str("} }");
        assert!(messages(method_too_long, &src).is_empty());
    }

    #[test]
    fn bodyless_and_short_methods_are_left_alone() {
        assert!(messages(method_too_long, "interface I { void f(); }").is_empty());
        assert!(messages(method_too_long, "class A { void f() {} }").is_empty());
        assert!(messages(method_too_long, "class A {}").is_empty());
    }

    #[test]
    fn test_classes_need_the_test_suffix() {
        assert_eq!(messages(test_class_suffix, "class Demo { @Test void f() {} }").len(), 1);
        assert_eq!(messages(test_class_suffix, "class Demo { @org.junit.Test void f() {} }").len(), 1);
        assert_eq!(messages(test_class_suffix, "class Demo { @ParameterizedTest void f() {} }").len(), 1);
    }

    #[test]
    fn properly_named_test_classes_are_fine() {
        assert!(messages(test_class_suffix, "class DemoTest { @Test void f() {} }").is_empty());
        assert!(messages(test_class_suffix, "class Demo { void f() {} }").is_empty());
        assert!(messages(test_class_suffix, "class Demo { @Override void f() {} }").is_empty());
    }

    #[test]
    fn only_the_innermost_test_class_is_flagged() {
        let src = "class Outer { static class Inner { @Test void f() {} } }";
        assert_eq!(messages(test_class_suffix, src).len(), 1);
        assert!(messages(test_class_suffix, "class Outer { static class InnerTest { @Test void f() {} } }").is_empty());
    }
}
