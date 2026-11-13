//! 快速修复：只修机械可判定的问题。
//!
//! 线程池该怎么建、集合换成哪个实现、方法怎么拆，都要人决定，所以不给修复。
//! 修复复用规则自己的诊断区间，不另写一套识别逻辑，免得两边判断不一致。

use crate::lex::Kind;
use crate::rules::{Ctx, RULES, seq};
use openidea_plugin::providers::CodeAction;
use openidea_plugin::{Range, TextEdit};

type Build = fn(&Ctx, usize, usize) -> Option<Vec<TextEdit>>;

struct Fix {
    rule: &'static str,
    title_zh: &'static str,
    title_en: &'static str,
    build: Build,
}

const FIXES: &[Fix] = &[
    Fix {
        rule: "p3c.constant.long-literal-suffix",
        title_zh: "把 long 后缀改成大写 L",
        title_en: "Use an uppercase L for the long suffix",
        build: upper_l,
    },
    Fix {
        rule: "p3c.other.date-pattern-year",
        title_zh: "把 YYYY 改成 yyyy",
        title_en: "Change YYYY to yyyy",
        build: lower_y,
    },
    Fix {
        rule: "p3c.other.current-time-millis",
        title_zh: "改用 System.currentTimeMillis()",
        title_en: "Use System.currentTimeMillis()",
        build: current_millis,
    },
    Fix {
        rule: "p3c.naming.array-brackets",
        title_zh: "把中括号移到类型后面",
        title_en: "Move the brackets next to the type",
        build: move_brackets,
    },
    Fix {
        rule: "p3c.oop.equals-literal-first",
        title_zh: "交换 equals 的调用方与参数",
        title_en: "Swap the equals receiver and argument",
        build: swap_equals,
    },
    Fix { rule: "p3c.flow.braces", title_zh: "补上大括号", title_en: "Add braces", build: add_braces },
];

/// 光标或选区所在行上的问题都给修复，与 IDE 的 Alt+Enter 一致。
pub fn actions(ctx: &Ctx, path: &str, selection: Range) -> Vec<CodeAction> {
    let settings = ctx.settings();
    let mut out = Vec::new();
    for rule in RULES {
        let Some(fix) = FIXES.iter().find(|f| f.rule == rule.id) else { continue };
        if !settings.allows(rule.group) {
            continue;
        }
        for d in (rule.check)(ctx) {
            if d.range.start.line > selection.end.line || selection.start.line > d.range.end.line {
                continue;
            }
            let (start, end) = (ctx.offset(d.range.start), ctx.offset(d.range.end));
            let Some(edits) = (fix.build)(ctx, start, end) else { continue };
            out.push(CodeAction {
                title: ctx.t(fix.title_zh, fix.title_en).into(),
                kind: Some("quickfix".into()),
                preferred: false,
                edits: vec![(path.to_string(), edits)],
                data: None,
            });
        }
    }
    out
}

fn edit(ctx: &Ctx, start: usize, end: usize, text: &str) -> TextEdit {
    TextEdit { range: ctx.range(start, end), text: text.into() }
}

fn upper_l(ctx: &Ctx, start: usize, end: usize) -> Option<Vec<TextEdit>> {
    Some(vec![edit(ctx, start, end, "L")])
}

fn lower_y(ctx: &Ctx, start: usize, end: usize) -> Option<Vec<TextEdit>> {
    Some(vec![edit(ctx, start, end, "yyyy")])
}

fn current_millis(ctx: &Ctx, start: usize, end: usize) -> Option<Vec<TextEdit>> {
    Some(vec![edit(ctx, start, end, "System.currentTimeMillis()")])
}

/// `String args[]` → `String[] args`：类型后插入，名字后删除，一次工作区编辑。
fn move_brackets(ctx: &Ctx, name_start: usize, _: usize) -> Option<Vec<TextEdit>> {
    let d = ctx.decl();
    let named = d
        .fields
        .iter()
        .map(|f| (f.name, f.array_after_name))
        .chain(d.locals.iter().map(|l| (l.name, l.array_after_name)))
        .chain(d.methods.iter().flat_map(|m| m.params.iter().map(|p| (p.name, p.array_after_name))));
    let (name, _) = named.into_iter().find(|(n, after)| *after && n.start == name_start)?;
    let i = ctx.code.partition_point(|t| t.start < name.start);
    let ty_end = ctx.code.get(i.checked_sub(1)?)?.end;
    // 名字后连续的 [] 一起搬走，多维数组才不会只搬一半。
    let mut j = i + 1;
    let mut dims = 0;
    while seq(ctx, j, &["[", "]"]) {
        j += 2;
        dims += 1;
    }
    if dims == 0 {
        return None;
    }
    Some(vec![edit(ctx, ty_end, ty_end, &"[]".repeat(dims)), edit(ctx, name.end, ctx.code[j - 1].end, "")])
}

/// `obj.equals("x")` → `"x".equals(obj)`。
fn swap_equals(ctx: &Ctx, method_start: usize, method_end: usize) -> Option<Vec<TextEdit>> {
    let i = ctx.code.partition_point(|t| t.start < method_start);
    let dot = i.checked_sub(1)?;
    let open = i + 1;
    let close = ctx.parens()[open]?;
    let recv_start = receiver_start(ctx, dot)?;
    let arg = ctx.next_token(ctx.code[open].end)?;
    if !matches!(arg.kind, Kind::String | Kind::Char) {
        return None;
    }
    // 实参必须是单独一个字面量：`"a" + b` 交换后改变语义，`equals("a", b)` 根本不是 Object.equals。
    if ctx.next_token(arg.end).map(|t| ctx.slice(t)) != Some(")") {
        return None;
    }
    let recv = &ctx.text[recv_start..ctx.code[dot].start];
    // super（含 A.super 这类限定形式）只能作方法限定符，不能作实参：`"x".equals(super)` / `"x".equals(A.super)` 编译不过。
    // 调用方最后一个 token 是 super 即为限定符-only；super.method() 链以 `)` 结尾，不受影响。
    if ctx.slice(&ctx.code[dot - 1]) == "super" {
        return None;
    }
    let method = &ctx.text[method_start..method_end];
    let literal = &ctx.text[arg.start..arg.end];
    Some(vec![edit(ctx, recv_start, ctx.code[close].end, &format!("{literal}.{method}({recv})"))])
}

/// 调用方表达式的起点：标识符、this/super/new、带括号的调用链与泛型限定（A.<T>value()）；
/// 扫到语句关键字或表达式边界即停。跨过一个 `.` 后若无法在其左侧确立完整主表达式
/// （如数组下标 `]`、字面量调用方），返回 None 而非已收集的后缀，避免产出残缺替换。
fn receiver_start(ctx: &Ctx, dot: usize) -> Option<usize> {
    let mut i = dot;
    let mut start = None;
    let mut pending_dot = false;
    while let Some(prev) = i.checked_sub(1) {
        let t = &ctx.code[prev];
        match ctx.slice(t) {
            ")" => {
                let open = ctx.parens().iter().position(|&c| c == Some(prev))?;
                start = Some(ctx.code[open].start);
                i = open;
                pending_dot = false;
            }
            // 泛型方法调用 A.<T>value()：连同 <T> 一起保留，否则 A.<T> 会落在替换区外、贴到字面量上。
            ">" => {
                let open = matching_angle_back(ctx, prev)?;
                start = Some(ctx.code[open].start);
                i = open;
                pending_dot = false;
            }
            "." => {
                i = prev;
                pending_dot = true;
            }
            _ if t.kind == Kind::Ident && !keyword_boundary(ctx.slice(t)) => {
                start = Some(t.start);
                i = prev;
                pending_dot = false;
            }
            _ => break,
        }
    }
    if pending_dot { None } else { start }
}

/// 从 `close` 处的 `>` 向左找匹配的 `<`（skip_generics 的反向）；找不到说明它是比较运算符而非泛型，返回 None。
fn matching_angle_back(ctx: &Ctx, close: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut j = close;
    loop {
        match ctx.slice(&ctx.code[j]) {
            ">" => depth += 1,
            "<" => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
        j = j.checked_sub(1)?;
    }
}

/// 关键字也被词法器标成 Ident，但只有 this/super/new 属于调用方；return/throw/yield 等是边界，纳入会产出非法代码。
fn keyword_boundary(s: &str) -> bool {
    const BOUNDARY: &[&str] = &[
        "abstract",
        "assert",
        "boolean",
        "break",
        "byte",
        "case",
        "catch",
        "char",
        "class",
        "const",
        "continue",
        "default",
        "do",
        "double",
        "else",
        "enum",
        "extends",
        "final",
        "finally",
        "float",
        "for",
        "goto",
        "if",
        "implements",
        "import",
        "instanceof",
        "int",
        "interface",
        "long",
        "native",
        "package",
        "private",
        "protected",
        "public",
        "return",
        "short",
        "static",
        "strictfp",
        "switch",
        "synchronized",
        "throw",
        "throws",
        "transient",
        "try",
        "void",
        "volatile",
        "while",
        "true",
        "false",
        "null",
        "var",
        "yield",
        "record",
        "sealed",
        "permits",
    ];
    BOUNDARY.contains(&s)
}

/// 给单行语句补大括号：区间到同一层的第一个 `;` 为止。
fn add_braces(ctx: &Ctx, kw_start: usize, _: usize) -> Option<Vec<TextEdit>> {
    let i = ctx.code.partition_point(|t| t.start < kw_start);
    // 从 `)` 或关键字之后开始替换，连同换行与缩进一起吃掉，大括号才留在同一行。
    let (body, anchor) = match ctx.slice(&ctx.code[i]) {
        "if" | "for" | "while" => {
            let close = ctx.parens().get(i + 1).copied().flatten()?;
            (close + 1, ctx.code[close].end)
        }
        _ => (i + 1, ctx.code[i].end),
    };
    let first = ctx.code.get(body)?;
    // 嵌套 if 的 else 归属会变，补大括号可能改变语义，交给人处理。
    if ctx.slice(first) == "if" {
        return None;
    }
    let mut depth = 0usize;
    let mut end = None;
    for j in body..ctx.code.len() {
        match ctx.slice(&ctx.code[j]) {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            ";" if depth == 0 => {
                end = Some(ctx.code[j].end);
                break;
            }
            _ => {}
        }
    }
    let end = end?;
    let text = format!(" {{ {} }}", ctx.text[first.start..end].trim());
    Some(vec![edit(ctx, anchor, end, &text)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::Position;
    use openidea_plugin::mock::MockHost;

    /// 把修复应用到原文，返回 (标题, 改后的全文)。
    fn applied(src: &str, line: u32) -> Vec<(String, String)> {
        let _mock = MockHost::install();
        let ctx = Ctx::new(src);
        let sel = Range::new(Position::new(line, 0), Position::new(line, 0));
        actions(&ctx, "/project/A.java", sel)
            .into_iter()
            .map(|a| {
                let mut text = src.to_string();
                let mut edits = a.edits[0].1.clone();
                // 从后往前应用，前面的偏移才不会失效。
                edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
                for e in edits {
                    let s = ctx.offset(e.range.start);
                    let t = ctx.offset(e.range.end);
                    text.replace_range(s..t, &e.text);
                }
                (a.title, text)
            })
            .collect()
    }

    fn one(src: &str, line: u32) -> String {
        let got = applied(src, line);
        assert_eq!(got.len(), 1, "{got:?}");
        got.into_iter().next().unwrap().1
    }

    #[test]
    fn the_long_suffix_becomes_uppercase() {
        assert_eq!(one("class A { long n = 2l; }", 0), "class A { long n = 2L; }");
        assert_eq!(one("class A { long n = 0x1fl; }", 0), "class A { long n = 0x1fL; }");
    }

    #[test]
    fn every_week_based_year_in_a_pattern_is_fixed() {
        let src = "class A { String p = \"YYYY-MM-dd YYYY\"; }";
        let got = applied(src, 0);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].1, "class A { String p = \"yyyy-MM-dd YYYY\"; }");
        assert_eq!(got[1].1, "class A { String p = \"YYYY-MM-dd yyyy\"; }");
    }

    #[test]
    fn new_date_get_time_becomes_current_time_millis() {
        assert_eq!(
            one("class A { long t = new Date().getTime(); }", 0),
            "class A { long t = System.currentTimeMillis(); }"
        );
        assert_eq!(
            one("class A { long t = new java.util.Date().getTime(); }", 0),
            "class A { long t = System.currentTimeMillis(); }"
        );
    }

    #[test]
    fn c_style_arrays_move_the_brackets_to_the_type() {
        assert_eq!(one("class A { void f(String args[]) {} }", 0), "class A { void f(String[] args) {} }");
        assert_eq!(one("class A { private int vals[]; }", 0), "class A { private int[] vals; }");
        assert_eq!(one("class A { void f() { int xs[] = null; } }", 0), "class A { void f() { int[] xs = null; } }");
        assert_eq!(one("class A { void f(String args[][]) {} }", 0), "class A { void f(String[][] args) {} }");
        assert_eq!(
            one("class A { void f(java.util.List<String> xs[]) {} }", 0),
            "class A { void f(java.util.List<String>[] xs) {} }"
        );
    }

    #[test]
    fn equals_swaps_the_literal_to_the_front() {
        assert_eq!(
            one("class A { void f() { obj.equals(\"test\"); } }", 0),
            "class A { void f() { \"test\".equals(obj); } }"
        );
        assert_eq!(
            one("class A { void f() { getObj().equalsIgnoreCase(\"x\"); } }", 0),
            "class A { void f() { \"x\".equalsIgnoreCase(getObj()); } }"
        );
        assert_eq!(
            one("class A { void f() { a.b.getC().equals('c'); } }", 0),
            "class A { void f() { 'c'.equals(a.b.getC()); } }"
        );
    }

    #[test]
    fn a_concatenated_argument_is_not_swapped() {
        assert!(applied("class A { void f() { obj.equals(\"a\" + b); } }", 0).is_empty());
        assert!(applied("class A { void f() { obj.equals(\"a\", b); } }", 0).is_empty());
    }

    #[test]
    fn a_return_keyword_stays_outside_the_swap() {
        assert_eq!(
            one("class A { boolean f(String obj) { return obj.equals(\"x\"); } }", 0),
            "class A { boolean f(String obj) { return \"x\".equals(obj); } }"
        );
        assert_eq!(
            one("class A { boolean f() { return a.b.getC().equals(\"x\"); } }", 0),
            "class A { boolean f() { return \"x\".equals(a.b.getC()); } }"
        );
        assert_eq!(
            one("class A { boolean f() { return getObj().equalsIgnoreCase(\"x\"); } }", 0),
            "class A { boolean f() { return \"x\".equalsIgnoreCase(getObj()); } }"
        );
    }

    #[test]
    fn new_and_this_receivers_stay_intact() {
        assert_eq!(
            one("class A { boolean f() { return new Foo().equals(\"x\"); } }", 0),
            "class A { boolean f() { return \"x\".equals(new Foo()); } }"
        );
        assert_eq!(
            one("class A { boolean f() { return this.equals(\"x\"); } }", 0),
            "class A { boolean f() { return \"x\".equals(this); } }"
        );
    }

    #[test]
    fn bare_super_is_omitted_but_a_super_chain_is_swapped() {
        // 裸 super 不能作实参，跳过；super.toString() 是合法调用方，照常交换。
        assert!(applied("class A { boolean f() { return super.equals(\"x\"); } }", 0).is_empty());
        assert_eq!(
            one("class A { boolean f() { return super.toString().equals(\"x\"); } }", 0),
            "class A { boolean f() { return \"x\".equals(super.toString()); } }"
        );
    }

    #[test]
    fn a_generic_method_receiver_is_kept_whole() {
        // 期望输出均为合法 Java：A 声明了 static <T> T value()，泛型限定连同调用方一起搬移。
        assert_eq!(
            one(
                "class A { static <T> T value() { return null; } boolean f() { return A.<String>value().equals(\"x\"); } }",
                0
            ),
            "class A { static <T> T value() { return null; } boolean f() { return \"x\".equals(A.<String>value()); } }"
        );
        assert_eq!(
            one(
                "class A { static <T> T value() { return null; } boolean f() { return A.<java.util.Map<String, Integer>>value().equals(\"x\"); } }",
                0
            ),
            "class A { static <T> T value() { return null; } boolean f() { return \"x\".equals(A.<java.util.Map<String, Integer>>value()); } }"
        );
    }

    #[test]
    fn a_literal_receiver_is_conservatively_left_alone() {
        // null 是字面量关键字而非可交换的调用方，宁可不给修复也不产出非法代码。
        assert!(applied("class A { boolean f() { return null.equals(\"x\"); } }", 0).is_empty());
    }

    #[test]
    fn an_unsupported_chain_root_is_omitted_not_partially_swapped() {
        // 数组下标 / 字面量调用方无法完整解析：宁可跳过，也不产出 args[0]."x".equals(trim()) 这类残缺替换。
        assert!(applied("class A { boolean f(String[] args) { return args[0].trim().equals(\"x\"); } }", 0).is_empty());
        assert!(applied("class A { boolean f() { return \" a \".trim().equals(\"x\"); } }", 0).is_empty());
    }

    #[test]
    fn a_parenthesized_array_root_is_swapped() {
        // 加括号后调用方是完整的括号表达式，可安全交换。
        assert_eq!(
            one("class A { boolean f(String[] args) { return (args[0]).trim().equals(\"x\"); } }", 0),
            "class A { boolean f(String[] args) { return \"x\".equals((args[0]).trim()); } }"
        );
    }

    #[test]
    fn qualified_super_is_omitted_but_its_chain_is_swapped() {
        // A.super 与裸 super 一样只是限定符，不能作实参；A.super.toString() 是合法调用方。
        assert!(applied("class A { class Inner { boolean f() { return A.super.equals(\"x\"); } } }", 0).is_empty());
        assert_eq!(
            one("class A { class Inner { boolean f() { return A.super.toString().equals(\"x\"); } } }", 0),
            "class A { class Inner { boolean f() { return \"x\".equals(A.super.toString()); } } }"
        );
    }

    #[test]
    fn an_identifier_ending_in_super_is_still_swapped() {
        // 仅当调用方最后一个 token 恰为 super 才跳过，mysuper 这类普通标识符不受影响。
        assert_eq!(
            one("class A { boolean f(String mysuper) { return mysuper.equals(\"x\"); } }", 0),
            "class A { boolean f(String mysuper) { return \"x\".equals(mysuper); } }"
        );
    }

    #[test]
    fn braces_wrap_the_single_statement() {
        assert_eq!(one("class A { void f() { if (ok) run(); } }", 0), "class A { void f() { if (ok) { run(); } } }");
        assert_eq!(
            one("class A { void f() { while (ok) run(); } }", 0),
            "class A { void f() { while (ok) { run(); } } }"
        );
        assert_eq!(
            one("class A { void f() { do run(); while (ok); } }", 0),
            "class A { void f() { do { run(); } while (ok); } }"
        );
        // if 与 else 各是一条问题，也就各有一条修复。
        let both = applied("class A { void f() { if (a) x(); else y(); } }", 0);
        assert_eq!(both.len(), 2);
        assert_eq!(both[0].1, "class A { void f() { if (a) { x(); } else y(); } }");
        assert_eq!(both[1].1, "class A { void f() { if (a) x(); else { y(); } } }");
        assert_eq!(
            one("class A { void f() { for (int i = 0; i < 9; i++) sum(i); } }", 0),
            "class A { void f() { for (int i = 0; i < 9; i++) { sum(i); } } }"
        );
    }

    #[test]
    fn the_outer_if_of_a_nested_pair_is_left_to_the_author() {
        // 外层补括号会把 else 抢过去；内层 if 与 else 各补各的，都不改变 else 的归属。
        let got = applied("class A { void f() { if (a) if (b) x(); else y(); } }", 0);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].1, "class A { void f() { if (a) if (b) { x(); } else y(); } }");
        assert_eq!(got[1].1, "class A { void f() { if (a) if (b) x(); else { y(); } } }");
    }

    #[test]
    fn a_multiline_body_keeps_its_indentation() {
        let src = "class A {\n    void f() {\n        if (ok)\n            run();\n    }\n}";
        let got = applied(src, 2);
        assert_eq!(got.len(), 1);
        assert!(got[0].1.contains("if (ok) { run(); }"), "{}", got[0].1);
    }

    #[test]
    fn fixes_follow_the_switches_and_the_cursor_line() {
        let mock = MockHost::install();
        let src = "class A {\n    long n = 2l;\n    long m = 3l;\n}";
        let ctx = Ctx::new(src);
        let on_line = Range::new(Position::new(1, 0), Position::new(1, 0));
        assert_eq!(actions(&ctx, "/project/A.java", on_line).len(), 1);

        mock.set_setting("group.constant", "false");
        assert!(actions(&Ctx::new(src), "/project/A.java", on_line).is_empty());
        mock.set_setting("group.constant", "true");
        mock.set_setting("enabled", "false");
        assert!(actions(&Ctx::new(src), "/project/A.java", on_line).is_empty());
    }

    #[test]
    fn chinese_text_before_a_fix_keeps_the_columns_right() {
        let src = "class A { String 说明 = \"中文\"; long n = 2l; }";
        assert_eq!(one(src, 0), "class A { String 说明 = \"中文\"; long n = 2L; }");
        let src = "class A { void f() { 对象.equals(\"测试\"); } }";
        assert_eq!(one(src, 0), "class A { void f() { \"测试\".equals(对象); } }");
    }
}
