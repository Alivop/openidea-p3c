//! 控制语句（手册·控制语句）。

use super::{Ctx, seq};
use openidea_plugin::{Diagnostic, Severity};
use std::collections::HashSet;

/// 手册·控制语句 1：if/else/for/while/do 必须使用大括号，即使只有一行。
pub fn braces(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        let word = ctx.slice(&ctx.code[i]);
        let body = match word {
            "if" | "for" | "while" => {
                if !seq(ctx, i + 1, &["("]) {
                    continue;
                }
                let Some(close) = ctx.parens()[i + 1] else { continue };
                // `do { … } while (cond);` 的分号是语法要求，不是缺大括号。
                if word == "while" && seq(ctx, close + 1, &[";"]) {
                    continue;
                }
                close + 1
            }
            // `else if` 是允许的写法。
            "else" if !seq(ctx, i + 1, &["if"]) => i + 1,
            "do" => i + 1,
            _ => continue,
        };
        if seq(ctx, body, &["{"]) {
            continue;
        }
        let message = if ctx.zh() {
            format!("{word} 语句必须使用大括号，即使只有一行代码，也要避免单行写法（手册·控制语句 1）")
        } else {
            format!(
                "A {word} statement must use braces, even for a single line of code; avoid the one-line form \
                 (Handbook: Flow Control 1)"
            )
        };
        out.push(ctx.diag(ctx.code[i].start, ctx.code[i].end, Severity::Warning, &message));
    }
    out
}

/// 手册·控制语句：finally 块中的 return 会吞掉 try 块的返回值与异常。
pub fn return_in_finally(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["finally", "{"]) {
            continue;
        }
        let Some(close) = ctx.braces()[i + 1] else { continue };
        for j in (i + 2)..close {
            if ctx.slice(&ctx.code[j]) == "return" {
                out.push(ctx.diag(
                    ctx.code[j].start,
                    ctx.code[j].end,
                    Severity::Error,
                    ctx.t(
                        "不要在 finally 块中使用 return：它会直接结束方法，\
                         使 try 块中的 return 与抛出的异常都被丢弃（手册·异常日志）",
                        "Do not return from a finally block: it ends the method immediately, discarding both the \
                         try block's return value and any thrown exception (Handbook: Exceptions & Logging)",
                    ),
                ));
            }
        }
    }
    out
}

/// 所有循环体的 token 区间，大括号除外。
fn loop_bodies(ctx: &Ctx) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        let body = match ctx.slice(&ctx.code[i]) {
            "for" | "while" => {
                if !seq(ctx, i + 1, &["("]) {
                    continue;
                }
                match ctx.parens()[i + 1] {
                    Some(close) => close + 1,
                    None => continue,
                }
            }
            "do" => i + 1,
            _ => continue,
        };
        if seq(ctx, body, &["{"])
            && let Some(close) = ctx.braces()[body]
        {
            out.push((body + 1, close));
        }
    }
    out
}

const MUTATORS: [&str; 6] = ["remove", "add", "clear", "addAll", "removeAll", "retainAll"];

/// 手册·集合处理：foreach 循环里不要 remove/add 元素，remove 请用 Iterator。
pub fn foreach_mutation(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["for", "("]) {
            continue;
        }
        let Some(close) = ctx.parens()[i + 1] else { continue };
        let Some(colon) = (i + 2..close).find(|&j| ctx.slice(&ctx.code[j]) == ":") else { continue };
        // 只处理 `for (T x : name)`：被迭代的表达式是单个标识符时才敢下结论。
        if colon + 2 != close || !seq(ctx, close + 1, &["{"]) {
            continue;
        }
        let name = ctx.slice(&ctx.code[colon + 1]);
        let Some(body_close) = ctx.braces()[close + 1] else { continue };
        for j in (close + 2)..body_close {
            if ctx.slice(&ctx.code[j]) != name || !seq(ctx, j + 1, &["."]) {
                continue;
            }
            let Some(m) = ctx.code.get(j + 2) else { continue };
            if MUTATORS.contains(&ctx.slice(m)) && seq(ctx, j + 3, &["("]) {
                let method = ctx.slice(m);
                let message = if ctx.zh() {
                    format!(
                        "不要在 foreach 循环里对迭代的集合调用 {method}，会抛 ConcurrentModificationException；\
                         删除元素请改用 Iterator 的 remove（手册·集合处理）"
                    )
                } else {
                    format!(
                        "Do not call {method} on the collection being iterated in a foreach loop: it throws \
                         ConcurrentModificationException; to remove elements use Iterator.remove \
                         (Handbook: Collections)"
                    )
                };
                out.push(ctx.diag(m.start, m.end, Severity::Error, &message));
            }
        }
    }
    out
}

/// 手册·其它：循环体内连接字符串用 StringBuilder.append，不要每次新建对象。
pub fn string_concat_in_loop(ctx: &Ctx) -> Vec<Diagnostic> {
    let d = ctx.decl();
    let strings: HashSet<&str> = d
        .locals
        .iter()
        .map(|l| (l.name.text(ctx), l.ty.as_str()))
        .chain(d.fields.iter().map(|f| (f.name.text(ctx), f.ty.as_str())))
        .chain(d.methods.iter().flat_map(|m| m.params.iter().map(|p| (p.name.text(ctx), p.ty.as_str()))))
        .filter(|(_, ty)| *ty == "String")
        .map(|(name, _)| name)
        .collect();
    if strings.is_empty() {
        return Vec::new();
    }
    let bodies = loop_bodies(ctx);
    if bodies.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for j in 0..ctx.code.len() {
        let name = ctx.slice(&ctx.code[j]);
        if ctx.code[j].kind != crate::lex::Kind::Ident || !strings.contains(name) {
            continue;
        }
        // `s += …` 与 `s = s + …` 是同一个问题。
        let concat = seq(ctx, j + 1, &["+="])
            || (seq(ctx, j + 1, &["="])
                && ctx.code.get(j + 2).is_some_and(|t| ctx.slice(t) == name)
                && seq(ctx, j + 3, &["+"]));
        if !concat || !bodies.iter().any(|&(s, e)| j >= s && j < e) {
            continue;
        }
        out.push(ctx.diag(
            ctx.code[j].start,
            ctx.code[j + 1].end,
            Severity::Warning,
            ctx.t(
                "循环体内连接字符串请用 StringBuilder 的 append：\
                 += 每次都会新建 StringBuilder 与 String 对象，浪费内存并影响性能（手册·其它）",
                "Concatenate strings inside a loop with StringBuilder.append: += creates a new StringBuilder and \
                 String every time, wasting memory and hurting performance (Handbook: Misc)",
            ),
        ));
    }
    out
}

/// 手册·控制语句：switch 必须带 default。只检查冒号形式的 switch 语句；
/// 箭头形式的 switch 表达式常靠类型穷尽性保证，跳过以免误报。
pub fn switch_default(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["switch", "("]) {
            continue;
        }
        let Some(paren_close) = ctx.parens()[i + 1] else { continue };
        let open = paren_close + 1;
        if !seq(ctx, open, &["{"]) {
            continue;
        }
        let Some(close) = ctx.braces()[open] else { continue };
        let mut has_default = false;
        let mut arrow = false;
        let mut depth = 0usize;
        for j in (open + 1)..close {
            match ctx.slice(&ctx.code[j]) {
                "{" => depth += 1,
                "}" => depth = depth.saturating_sub(1),
                "case" | "default" if depth == 0 => {
                    has_default |= ctx.slice(&ctx.code[j]) == "default";
                    arrow |= label_is_arrow(ctx, j + 1, close);
                }
                _ => {}
            }
        }
        if has_default || arrow {
            continue;
        }
        out.push(ctx.diag(
            ctx.code[i].start,
            ctx.code[i].end,
            Severity::Warning,
            ctx.t(
                "switch 语句必须包含 default 分支，即使其中什么代码也没有，也要写上以明确兜底逻辑（手册·控制语句）",
                "A switch statement must include a default branch, even if it is empty, to make the fallback \
                 explicit (Handbook: Flow Control)",
            ),
        ));
    }
    out
}

/// 从 case/default 标签之后扫到它的结束符：`->` 是箭头形式，`:` 是冒号形式。
fn label_is_arrow(ctx: &Ctx, start: usize, end: usize) -> bool {
    let mut depth = 0usize;
    for j in start..end {
        match ctx.slice(&ctx.code[j]) {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            "->" if depth == 0 => return true,
            ":" | "{" if depth == 0 => return false,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn count(rule: fn(&Ctx) -> Vec<Diagnostic>, src: &str) -> usize {
        let _mock = MockHost::install();
        rule(&Ctx::new(src)).len()
    }

    fn messages(rule: fn(&Ctx) -> Vec<Diagnostic>, src: &str) -> Vec<String> {
        let _mock = MockHost::install();
        rule(&Ctx::new(src)).into_iter().map(|d| d.message).collect()
    }

    #[test]
    fn single_line_bodies_are_flagged_for_every_keyword() {
        let src = "class A { void f() { if (a) b(); for (;;) c(); while (d) e(); do g(); while (d); } }";
        assert_eq!(count(braces, src), 4);
        let src = "class A { void f() { if (a) { b(); } else { c(); } } }";
        assert_eq!(count(braces, src), 0);
    }

    #[test]
    fn else_if_and_do_while_tails_are_not_missing_braces() {
        assert_eq!(count(braces, "class A { void f() { if (a) { b(); } else if (c) { d(); } else { e(); } } }"), 0);
        assert_eq!(count(braces, "class A { void f() { do { b(); } while (a); } }"), 0);
        assert_eq!(count(braces, "class A { void f() { if (a) b(); else c(); } }"), 2);
    }

    #[test]
    fn keywords_inside_literals_and_comments_are_ignored() {
        assert_eq!(count(braces, "class A { String s = \"if (a) b();\"; // while (c) d();\n/* for (;;) e(); */ }"), 0);
        assert_eq!(count(braces, "class A { void f() { switch (a) { case 1: b(); } } }"), 0);
    }

    #[test]
    fn return_in_finally_is_an_error() {
        let _mock = MockHost::install();
        let src = "class A { int f() { try { return 1; } finally { return 2; } } }";
        let got = return_in_finally(&Ctx::new(src));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].severity, Severity::Error);
        assert_eq!(count(return_in_finally, "class A { int f() { try { return 1; } finally { g(); } } }"), 0);
        assert_eq!(count(return_in_finally, "class A { int f() { if (a) { return 1; } } }"), 0);
    }

    #[test]
    fn nested_returns_in_finally_are_all_reported() {
        let src = "class A { int f() { try { g(); } finally { if (a) { return 1; } return 2; } } }";
        assert_eq!(count(return_in_finally, src), 2);
    }

    #[test]
    fn foreach_mutation_is_flagged_but_iterator_remove_is_not() {
        let src = "class A { void f() { for (String s : list) { list.remove(s); } } }";
        assert_eq!(count(foreach_mutation, src), 1);
        let src =
            "class A { void f() { for (Iterator<String> it = list.iterator(); it.hasNext();) { it.remove(); } } }";
        assert_eq!(count(foreach_mutation, src), 0);
        assert_eq!(count(foreach_mutation, "class A { void f() { for (String s : list) { other.remove(s); } } }"), 0);
    }

    #[test]
    fn foreach_read_only_and_remove_if_are_left_alone() {
        assert_eq!(count(foreach_mutation, "class A { void f() { for (String s : list) { g(s); } } }"), 0);
        assert_eq!(count(foreach_mutation, "class A { void f() { list.removeIf(s -> s.isEmpty()); } }"), 0);
        assert_eq!(count(foreach_mutation, "class A { void f() { for (int i = 0; i < 9; i++) { list.add(i); } } }"), 0);
    }

    #[test]
    fn every_mutator_is_covered() {
        for m in MUTATORS {
            let src = format!("class A {{ void f() {{ for (String s : list) {{ list.{m}(s); }} }} }}");
            assert_eq!(count(foreach_mutation, &src), 1, "{m}");
        }
    }

    #[test]
    fn string_concat_in_loops_is_flagged_in_both_spellings() {
        let src = "class A { void f() { String s = \"\"; for (int i = 0; i < 9; i++) { s += \"x\"; } } }";
        assert_eq!(count(string_concat_in_loop, src), 1);
        let src = "class A { void f() { String s = \"\"; while (a) { s = s + \"x\"; } } }";
        assert_eq!(count(string_concat_in_loop, src), 1);
        let src = "class A { void f() { String s = \"\"; s += \"x\"; } }";
        assert_eq!(count(string_concat_in_loop, src), 0);
    }

    #[test]
    fn string_builder_and_non_strings_are_left_alone() {
        let src = "class A { void f() { StringBuilder sb = new StringBuilder(); for (;;) { sb.append(\"x\"); } } }";
        assert_eq!(count(string_concat_in_loop, src), 0);
        let src = "class A { void f() { int n = 0; for (;;) { n += 1; } } }";
        assert_eq!(count(string_concat_in_loop, src), 0);
        assert_eq!(count(string_concat_in_loop, "class A { void f() { for (;;) { g(); } } }"), 0);
    }

    #[test]
    fn nested_loops_report_each_concat_once() {
        let src = "class A { void f() { String s = \"\"; for (;;) { while (a) { s += \"x\"; } } } }";
        assert_eq!(count(string_concat_in_loop, src), 1);
        assert_eq!(messages(string_concat_in_loop, src).len(), 1);
    }

    #[test]
    fn switch_without_default_is_flagged() {
        assert_eq!(count(switch_default, "class A { void f() { switch (x) { case 1: break; } } }"), 1);
        assert_eq!(count(switch_default, "class A { void f() { switch (x) { case 1: break; case 2: break; } } }"), 1);
    }

    #[test]
    fn switch_with_default_is_fine() {
        assert_eq!(count(switch_default, "class A { void f() { switch (x) { case 1: break; default: } } }"), 0);
        assert_eq!(count(switch_default, "class A { void f() { switch (x) { default: break; } } }"), 0);
    }

    #[test]
    fn arrow_switch_expressions_are_skipped() {
        assert_eq!(count(switch_default, "class A { int f() { return switch (x) { case 1 -> 1; case 2 -> 2; }; } }"), 0);
    }

    #[test]
    fn nested_switches_are_checked_independently() {
        let src = "class A { void f() { switch (x) { case 1: switch (y) { case 2: break; } break; default: } } }";
        assert_eq!(count(switch_default, src), 1);
    }

    #[test]
    fn a_lambda_arrow_in_a_colon_case_is_not_an_arrow_switch() {
        let src = "class A { void f() { switch (x) { case 1: r = () -> {}; break; } } }";
        assert_eq!(count(switch_default, src), 1);
    }
}
