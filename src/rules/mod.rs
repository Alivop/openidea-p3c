//! Rule registry.
//!
//! Rules read the token stream and report byte spans; only here do spans become
//! editor positions and rule ids. The diagnostics provider runs on the editing
//! path under a ~50 ms budget, so rules do no I/O: the host calls allowed are
//! reading settings and the UI language, both cached per document in `Ctx`.

use crate::lex::{Kind, LineIndex, Token, tokenize};
use crate::settings::{Group, Settings};
use openidea_plugin::{Diagnostic, Range, Severity, host};
use std::cell::OnceCell;

mod collection;
mod comment;
mod concurrent;
mod constant;
pub mod decl;
mod exception;
mod flow;
mod naming;
mod oop;
mod other;

pub struct Rule {
    /// Stable id shown in the Problems view; the host caps it at 128 bytes.
    pub id: &'static str,
    pub group: Group,
    pub check: fn(&Ctx) -> Vec<Diagnostic>,
}

pub struct Ctx<'a> {
    pub text: &'a str,
    pub tokens: Vec<Token>,
    /// Code tokens only: literals and comments filtered out, source order kept.
    pub code: Vec<Token>,
    lines: LineIndex,
    /// Parsed once and shared: a dozen rules need the same declarations.
    decl: OnceCell<decl::Decl>,
    parens: OnceCell<Vec<Option<usize>>>,
    braces: OnceCell<Vec<Option<usize>>>,
    settings: OnceCell<Settings>,
    /// 宿主不翻运行时拼的文案，语言判定每文档只问一次：5000 条诊断不能每条都跨宿主。
    zh: OnceCell<bool>,
}

impl<'a> Ctx<'a> {
    pub fn new(text: &'a str) -> Self {
        let tokens = tokenize(text);
        let code = tokens.iter().copied().filter(|t| t.kind.is_code()).collect();
        Self {
            text,
            tokens,
            code,
            lines: LineIndex::new(text),
            decl: OnceCell::new(),
            parens: OnceCell::new(),
            braces: OnceCell::new(),
            settings: OnceCell::new(),
            zh: OnceCell::new(),
        }
    }

    pub fn settings(&self) -> &Settings {
        self.settings.get_or_init(Settings::load)
    }

    /// 中文 locale（zh、zh-CN、zh-TW 都算）下用中文文案，否则用英文源文案。
    pub fn zh(&self) -> bool {
        *self.zh.get_or_init(|| host::locale().starts_with("zh"))
    }

    /// 诊断与快速修复标题的双语选择；带插值的文案在各规则里分支后用 `format!`。
    pub fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        if self.zh() { zh } else { en }
    }

    pub fn decl(&self) -> &decl::Decl {
        self.decl.get_or_init(|| decl::parse(self))
    }

    /// 每个 `(` 配对的 `)` 索引；未闭合为 `None`。
    pub fn parens(&self) -> &[Option<usize>] {
        self.parens.get_or_init(|| matching(self, "(", ")"))
    }

    /// 每个 `{` 配对的 `}` 索引；未闭合为 `None`。
    pub fn braces(&self) -> &[Option<usize>] {
        self.braces.get_or_init(|| matching(self, "{", "}"))
    }

    pub fn slice(&self, t: &Token) -> &'a str {
        &self.text[t.start..t.end]
    }

    pub fn line(&self, offset: usize) -> u32 {
        self.lines.position(self.text, offset).line
    }

    pub fn range(&self, start: usize, end: usize) -> Range {
        Range::new(self.lines.position(self.text, start), self.lines.position(self.text, end))
    }

    /// 编辑器坐标回到字节偏移，快速修复用它把诊断区间还原成可编辑的文本区间。
    pub fn offset(&self, pos: openidea_plugin::Position) -> usize {
        self.lines.offset(self.text, pos)
    }

    pub fn diag(&self, start: usize, end: usize, severity: Severity, message: &str) -> Diagnostic {
        Diagnostic::new(self.range(start, end), severity, message)
    }

    /// Tokens of a literal or comment, for rules that read them rather than skip them.
    pub fn of_kind(&self, kind: Kind) -> impl Iterator<Item = &Token> {
        self.tokens.iter().filter(move |t| t.kind == kind)
    }

    /// 偏移之后的第一个 token，含字面量与注释。
    pub fn next_token(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.tokens.partition_point(|t| t.start < offset))
    }

    /// 紧邻偏移之前的 token，含字面量与注释。
    pub fn prev_token(&self, offset: usize) -> Option<&Token> {
        let i = self.tokens.partition_point(|t| t.end <= offset);
        i.checked_sub(1).and_then(|i| self.tokens.get(i))
    }
}

/// Whether the code tokens at `i` read exactly `words`.
pub fn seq(ctx: &Ctx, i: usize, words: &[&str]) -> bool {
    words.iter().enumerate().all(|(k, w)| ctx.code.get(i + k).is_some_and(|t| ctx.slice(t) == *w))
}

/// A numeric literal that denotes a float/double, not an integer. Hex floats need
/// a `p` exponent, so a trailing `F` in `0x1F` is a hex digit, not a suffix.
pub fn is_floating_literal(s: &str) -> bool {
    if s.starts_with("0x") || s.starts_with("0X") {
        return s.contains('.') || s.contains('p') || s.contains('P');
    }
    if s.starts_with("0b") || s.starts_with("0B") {
        return false;
    }
    s.contains('.') || s.contains('e') || s.contains('E') || s.ends_with(|c| matches!(c, 'f' | 'F' | 'd' | 'D'))
}

/// Index just past the generic argument list whose `<` is at `open`.
pub fn skip_generics(ctx: &Ctx, open: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open;
    while i < ctx.code.len() {
        match ctx.slice(&ctx.code[i]) {
            "<" => depth += 1,
            ">" => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// Simple name of the annotation whose `@` sits at `at`, plus the index after the
/// name. Handles qualified spellings like `@org.junit.jupiter.api.Test`.
pub fn annotation_name<'a>(ctx: &Ctx<'a>, at: usize) -> Option<(&'a str, usize)> {
    let mut i = at + 1;
    let mut name = ctx.slice(ctx.code.get(i).filter(|t| t.kind == Kind::Ident)?);
    i += 1;
    while seq(ctx, i, &["."]) && ctx.code.get(i + 1).is_some_and(|t| t.kind == Kind::Ident) {
        name = ctx.slice(&ctx.code[i + 1]);
        i += 2;
    }
    Some((name, i))
}

/// Matching close index for every open bracket, in code-token order.
fn matching(ctx: &Ctx, open: &str, close: &str) -> Vec<Option<usize>> {
    let mut pairs = vec![None; ctx.code.len()];
    let mut stack = Vec::new();
    for (i, t) in ctx.code.iter().enumerate() {
        match ctx.slice(t) {
            w if w == open => stack.push(i),
            w if w == close => {
                if let Some(o) = stack.pop() {
                    pairs[o] = Some(i);
                }
            }
            _ => {}
        }
    }
    pairs
}

pub const RULES: &[Rule] = &[
    Rule { id: "p3c.constant.long-literal-suffix", group: Group::Constant, check: constant::long_literal_suffix },
    Rule { id: "p3c.other.date-pattern-year", group: Group::Other, check: other::date_pattern_year },
    Rule { id: "p3c.other.current-time-millis", group: Group::Other, check: other::current_time_millis },
    Rule { id: "p3c.other.apache-beanutils", group: Group::Other, check: other::apache_beanutils },
    Rule { id: "p3c.concurrent.executors", group: Group::Concurrent, check: concurrent::executors },
    Rule { id: "p3c.concurrent.new-thread", group: Group::Concurrent, check: concurrent::new_thread },
    Rule { id: "p3c.concurrent.timer", group: Group::Concurrent, check: concurrent::timer },
    Rule { id: "p3c.collection.to-array", group: Group::Collection, check: collection::to_array },
    Rule { id: "p3c.naming.no-edge-symbol", group: Group::Naming, check: naming::no_edge_symbol },
    Rule { id: "p3c.naming.no-chinese", group: Group::Naming, check: naming::no_chinese },
    Rule { id: "p3c.naming.class-upper-camel", group: Group::Naming, check: naming::class_upper_camel },
    Rule { id: "p3c.naming.lower-camel", group: Group::Naming, check: naming::lower_camel_names },
    Rule { id: "p3c.naming.constant-upper-snake", group: Group::Naming, check: naming::constant_upper_snake },
    Rule { id: "p3c.naming.abstract-and-exception", group: Group::Naming, check: naming::abstract_and_exception_names },
    Rule { id: "p3c.naming.array-brackets", group: Group::Naming, check: naming::array_brackets },
    Rule { id: "p3c.naming.boolean-no-is-prefix", group: Group::Naming, check: naming::boolean_no_is_prefix },
    Rule { id: "p3c.naming.package-lowercase", group: Group::Naming, check: naming::package_lowercase },
    Rule { id: "p3c.naming.method-too-long", group: Group::Naming, check: naming::method_too_long },
    Rule { id: "p3c.flow.braces", group: Group::Flow, check: flow::braces },
    Rule { id: "p3c.flow.return-in-finally", group: Group::Flow, check: flow::return_in_finally },
    Rule { id: "p3c.flow.foreach-mutation", group: Group::Flow, check: flow::foreach_mutation },
    Rule { id: "p3c.flow.string-concat-in-loop", group: Group::Flow, check: flow::string_concat_in_loop },
    Rule { id: "p3c.oop.equals-literal-first", group: Group::Oop, check: oop::equals_literal_first },
    Rule {
        id: "p3c.collection.arrays-aslist-mutation",
        group: Group::Collection,
        check: collection::arrays_aslist_mutation,
    },
    Rule { id: "p3c.concurrent.static-date-format", group: Group::Concurrent, check: concurrent::static_date_format },
    Rule { id: "p3c.other.pattern-compile-in-method", group: Group::Other, check: other::pattern_compile_in_method },
    Rule { id: "p3c.comment.abstract-method-javadoc", group: Group::Comment, check: comment::abstract_method_javadoc },
    Rule { id: "p3c.comment.enum-constant-javadoc", group: Group::Comment, check: comment::enum_constant_javadoc },
    Rule { id: "p3c.comment.class-author", group: Group::Comment, check: comment::class_author },
    Rule { id: "p3c.naming.test-class-suffix", group: Group::Naming, check: naming::test_class_suffix },
    Rule { id: "p3c.concurrent.static-random", group: Group::Concurrent, check: concurrent::static_random },
    Rule { id: "p3c.collection.init-capacity", group: Group::Collection, check: collection::init_capacity },
    Rule { id: "p3c.collection.sublist-cast", group: Group::Collection, check: collection::sublist_cast },
    Rule { id: "p3c.flow.switch-default", group: Group::Flow, check: flow::switch_default },
    Rule { id: "p3c.oop.bigdecimal-double", group: Group::Oop, check: oop::bigdecimal_double },
    Rule { id: "p3c.other.math-random-cast", group: Group::Other, check: other::math_random_cast },
    Rule { id: "p3c.other.float-equality", group: Group::Other, check: other::float_equality },
    Rule {
        id: "p3c.exception.transactional-rollback",
        group: Group::Exception,
        check: exception::transactional_rollback,
    },
];

/// The host keeps at most 5,000 diagnostics per provider and document.
const MAX_DIAGNOSTICS: usize = 5_000;

pub fn analyze(text: &str) -> Vec<Diagnostic> {
    let ctx = Ctx::new(text);
    let settings = ctx.settings();
    let mut out: Vec<Diagnostic> = RULES
        .iter()
        .filter(|r| settings.allows(r.group))
        .flat_map(|r| {
            (r.check)(&ctx).into_iter().map(|mut d| {
                d.code = Some(r.id.to_string());
                d
            })
        })
        .collect();
    out.sort_by_key(|d| (d.range.start.line, d.range.start.character, d.range.end.character));
    out.truncate(MAX_DIAGNOSTICS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_fit_the_host_limit_and_are_unique() {
        let mut ids: Vec<&str> = RULES.iter().map(|r| r.id).collect();
        assert!(ids.iter().all(|id| id.len() <= 128 && id.starts_with("p3c.")));
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn analyze_stamps_the_owning_rule_id() {
        let _mock = openidea_plugin::mock::MockHost::install();
        let got = analyze("long n = 2l; Executors.newCachedThreadPool();");
        let ids: Vec<&str> = got.iter().filter_map(|d| d.code.as_deref()).collect();
        assert_eq!(ids, ["p3c.constant.long-literal-suffix", "p3c.concurrent.executors"]);
    }

    #[test]
    fn results_are_ordered_by_position() {
        let _mock = openidea_plugin::mock::MockHost::install();
        let got = analyze("class A { void f() { new Thread(null); long n = 2l; } }");
        assert_eq!(got.len(), 2);
        assert!(got[0].range.start <= got[1].range.start);
    }

    /// 手动核对：`cargo test verification_fixture -- --ignored --nocapture`。
    /// 打印 fixtures/Verification.java 的期望清单，供在 IDE 里逐条对照。
    #[test]
    #[ignore]
    fn verification_fixture() {
        let _mock = openidea_plugin::mock::MockHost::install();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/Verification.java");
        let text = std::fs::read_to_string(path).expect("fixtures/Verification.java");
        let got = analyze(&text);
        for d in &got {
            println!(
                "{}:{}  {}  {}",
                d.range.start.line + 1,
                d.range.start.character + 1,
                d.code.as_deref().unwrap_or("-"),
                d.message
            );
        }
        println!("total: {}", got.len());
    }

    /// 误报抽查：`P3C_SWEEP_DIRS=/path/a:/path/b cargo test sweep -- --ignored --nocapture`。
    /// 按规则汇总真实项目的命中数，每条规则留 3 个样本供人工判真假。
    #[test]
    #[ignore]
    fn sweep_real_projects() {
        use std::collections::BTreeMap;
        use std::path::{Path, PathBuf};

        fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if path.is_dir() {
                    if !matches!(name.as_str(), "target" | "build" | "node_modules" | ".git" | ".idea") {
                        collect(&path, out);
                    }
                } else if name.ends_with(".java") {
                    out.push(path);
                }
            }
        }

        let _mock = openidea_plugin::mock::MockHost::install();
        let dirs = std::env::var("P3C_SWEEP_DIRS").unwrap_or_default();
        if dirs.is_empty() {
            println!("set P3C_SWEEP_DIRS=/path/a:/path/b");
            return;
        }
        let mut files = Vec::new();
        for dir in dirs.split(':').filter(|d| !d.is_empty()) {
            collect(Path::new(dir), &mut files);
        }
        files.sort();

        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut slowest = (0u128, String::new());
        let (mut hit_files, mut total, mut read_fail) = (0, 0, 0);
        for path in &files {
            let Ok(text) = std::fs::read_to_string(path) else {
                read_fail += 1;
                continue;
            };
            let started = std::time::Instant::now();
            let got = analyze(&text);
            let ms = started.elapsed().as_millis();
            if ms > slowest.0 {
                slowest = (ms, path.display().to_string());
            }
            if !got.is_empty() {
                hit_files += 1;
            }
            total += got.len();
            let lines: Vec<&str> = text.lines().collect();
            for d in got {
                let id = d.code.unwrap_or_else(|| "-".into());
                *counts.entry(id.clone()).or_default() += 1;
                let bucket = samples.entry(id).or_default();
                if bucket.len() < 3 {
                    let line = d.range.start.line as usize;
                    let src = lines.get(line).map_or("", |l| l.trim());
                    bucket.push(format!("{}:{}  {src}", path.display(), line + 1));
                }
            }
        }

        println!("files: {}  unreadable: {read_fail}  with problems: {hit_files}  diagnostics: {total}", files.len());
        println!("slowest: {} ms  {}", slowest.0, slowest.1);
        let mut ranked: Vec<(&String, &usize)> = counts.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (id, n) in ranked {
            println!("\n{n:>5}  {id}");
            for s in &samples[id] {
                println!("       {s}");
            }
        }
        let quiet: Vec<&str> = RULES.iter().map(|r| r.id).filter(|id| !counts.contains_key(*id)).collect();
        println!("\nnever fired ({}): {}", quiet.len(), quiet.join(", "));
    }

    /// 手动计时：`cargo test --release large_file -- --ignored --nocapture`。
    /// 宿主给诊断 provider 的软预算是 50 ms，超了结果会被丢弃。
    #[test]
    #[ignore]
    fn large_file_timing() {
        let _mock = openidea_plugin::mock::MockHost::install();
        let mut src = String::from(
            "package com.example.service;\n\nimport java.util.*;\n\npublic class OrderService {\n\
             \x20   private static final int MAX_RETRY = 3;\n",
        );
        for i in 0..120 {
            src.push_str(&format!(
                r#"
    /** 处理订单 {i} */
    public String handleOrder{i}(List<String> items, int count) {{
        String result = "";
        Map<String, Object> cache = new HashMap<>();
        for (String item : items) {{
            if (item == null) {{
                continue;
            }}
            result += item;
            cache.put(item, count);
        }}
        while (count > 0) {{
            count--;
        }}
        try {{
            return result.trim();
        }} finally {{
            log("done");
        }}
    }}
"#
            ));
        }
        src.push_str("}\n");

        let lines = src.lines().count();
        let start = std::time::Instant::now();
        let got = analyze(&src);
        let elapsed = start.elapsed();
        println!("{lines} 行 / {} 字节 / {} 条诊断 / {elapsed:?}", src.len(), got.len());
    }
}
