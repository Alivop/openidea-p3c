//! Shallow Java declaration parsing over the code token stream.
//!
//! Enough structure for the handbook's naming, comment and size rules: types,
//! methods, parameters, fields, enum constants and locals, each with its
//! modifiers, body span and attached Javadoc. No type resolution — the plugin
//! API exposes no semantic model, so rules stay syntactic.

use super::Ctx;
use crate::lex::{Kind, Token};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    fn of(t: &Token) -> Self {
        Self { start: t.start, end: t.end }
    }

    pub fn text<'t>(&self, ctx: &'t Ctx) -> &'t str {
        &ctx.text[self.start..self.end]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeKind {
    Class,
    Interface,
    Enum,
    Record,
}

pub struct TypeDecl {
    pub kind: TypeKind,
    pub name: Span,
    pub modifiers: Vec<Span>,
    /// Simple name of the `extends` target, for the exception-suffix rule.
    pub superclass: Option<String>,
    pub javadoc: Option<Span>,
    /// Interior code-token indices of the body, braces excluded.
    pub body: Option<(usize, usize)>,
}

pub struct Param {
    pub name: Span,
    pub ty: String,
    /// `String args[]` rather than `String[] args`.
    pub array_after_name: bool,
}

pub struct MethodDecl {
    pub name: Span,
    pub modifiers: Vec<Span>,
    pub params: Vec<Param>,
    /// Interior token indices of the body, braces excluded.
    pub body: Option<(usize, usize)>,
    pub javadoc: Option<Span>,
    pub in_interface: bool,
    /// 构造方法必须与类同名，命名规则不能要求它改成 lowerCamelCase。
    pub constructor: bool,
}

pub struct FieldDecl {
    pub name: Span,
    pub modifiers: Vec<Span>,
    pub ty: String,
    pub array_after_name: bool,
    /// 接口字段隐式 public static final，源码里看不到修饰符也是常量。
    pub in_interface: bool,
}

pub struct LocalDecl {
    pub name: Span,
    pub ty: String,
    pub array_after_name: bool,
}

pub struct EnumConstant {
    pub name: Span,
    pub javadoc: Option<Span>,
}

#[derive(Default)]
pub struct Decl {
    pub package: Option<Span>,
    pub types: Vec<TypeDecl>,
    pub methods: Vec<MethodDecl>,
    pub fields: Vec<FieldDecl>,
    pub locals: Vec<LocalDecl>,
    pub enum_constants: Vec<EnumConstant>,
}

pub fn has_modifier(ctx: &Ctx, modifiers: &[Span], word: &str) -> bool {
    modifiers.iter().any(|m| m.text(ctx) == word)
}

const MODIFIERS: [&str; 12] = [
    "public",
    "protected",
    "private",
    "static",
    "final",
    "abstract",
    "synchronized",
    "native",
    "transient",
    "volatile",
    "default",
    "strictfp",
];

/// Statement keywords: a declaration's type never starts with one.
const NON_TYPE: [&str; 18] = [
    "new", "return", "throw", "if", "for", "while", "switch", "try", "catch", "finally", "do", "else", "assert",
    "break", "continue", "this", "super", "case",
];

pub fn parse(ctx: &Ctx) -> Decl {
    let pairs = ctx.braces().to_vec();
    let docs = ctx.tokens.iter().filter(|t| t.kind == Kind::Javadoc).map(Span::of).collect();
    let bounds = ctx.code.iter().filter(|t| matches!(ctx.slice(t), ";" | "{" | "}")).map(|t| t.start).collect();
    let commas = ctx.code.iter().filter(|t| ctx.slice(t) == ",").map(|t| t.start).collect();
    Parser { ctx, pairs, docs, bounds, commas, type_names: Vec::new(), out: Decl::default() }.run()
}

struct Parser<'a, 'b> {
    ctx: &'a Ctx<'b>,
    pairs: Vec<Option<usize>>,
    /// Javadoc spans in source order, for binary search.
    docs: Vec<Span>,
    /// Byte offsets of `;`, `{`, `}`, for binary search.
    bounds: Vec<usize>,
    /// Enum constants are separated by commas, which also end a Javadoc's reach.
    commas: Vec<usize>,
    /// Names of the types being parsed, innermost last: a member sharing the
    /// innermost name is a constructor.
    type_names: Vec<&'b str>,
    out: Decl,
}

impl<'a, 'b> Parser<'a, 'b> {
    fn word(&self, i: usize) -> &'b str {
        self.ctx.slice(&self.ctx.code[i])
    }

    fn run(mut self) -> Decl {
        let mut i = 0;
        while i < self.ctx.code.len() {
            if self.word(i) == "package" {
                let mut j = i + 1;
                while j < self.ctx.code.len() && self.word(j) != ";" {
                    j += 1;
                }
                if j > i + 1 {
                    self.out.package = Some(Span { start: self.ctx.code[i + 1].start, end: self.ctx.code[j - 1].end });
                }
                i = j + 1;
                continue;
            }
            if let Some(kind) = self.type_keyword(i) {
                i = self.type_decl(i, kind);
                continue;
            }
            i += 1;
        }
        self.out
    }

    /// `class` is also a literal suffix (`Foo.class`), which is not a declaration.
    fn type_keyword(&self, i: usize) -> Option<TypeKind> {
        if i > 0 && self.word(i - 1) == "." {
            return None;
        }
        match self.word(i) {
            "class" => Some(TypeKind::Class),
            "interface" => Some(TypeKind::Interface),
            "enum" => Some(TypeKind::Enum),
            "record" => Some(TypeKind::Record),
            _ => None,
        }
    }

    fn type_decl(&mut self, kw: usize, kind: TypeKind) -> usize {
        let code = &self.ctx.code;
        let Some(name_tok) = code.get(kw + 1) else { return kw + 1 };
        if name_tok.kind != Kind::Ident {
            return kw + 1;
        }
        let start = self.decl_start(kw);
        let modifiers = self.modifiers(start, kw);

        let mut superclass = None;
        let mut j = kw + 2;
        let mut paren = 0usize;
        while j < code.len() {
            match self.word(j) {
                "(" => paren += 1,
                ")" => paren = paren.saturating_sub(1),
                "{" | ";" if paren == 0 => break,
                "extends" if paren == 0 && superclass.is_none() => {
                    superclass = code.get(j + 1).map(|t| self.ctx.slice(t).to_string());
                }
                _ => {}
            }
            j += 1;
        }
        let javadoc = self.javadoc_before(code[start].start);
        self.out.types.push(TypeDecl { kind, name: Span::of(name_tok), modifiers, superclass, javadoc, body: None });
        let type_index = self.out.types.len() - 1;

        // A record's header components are its fields.
        if kind == TypeKind::Record && code.get(kw + 2).is_some_and(|t| self.ctx.slice(t) == "(") {
            let open = kw + 2;
            let close = self.skip_balanced(open, "(", ")");
            for p in self.params(open + 1, close.saturating_sub(1)) {
                self.out.fields.push(FieldDecl {
                    name: p.name,
                    modifiers: Vec::new(),
                    ty: p.ty,
                    array_after_name: p.array_after_name,
                    in_interface: false,
                });
            }
        }

        if j < code.len() && self.word(j) == "{" {
            let close = self.pairs[j].unwrap_or(code.len().saturating_sub(1));
            self.out.types[type_index].body = Some((j + 1, close));
            let body = if kind == TypeKind::Enum { self.enum_constants(j + 1, close) } else { j + 1 };
            self.type_names.push(self.ctx.slice(name_tok));
            self.members(body, close, kind == TypeKind::Interface);
            self.type_names.pop();
            return close + 1;
        }
        j + 1
    }

    /// Walks back over modifiers and annotations to the declaration's first token.
    fn decl_start(&self, kw: usize) -> usize {
        let mut i = kw;
        while i > 0 {
            let w = self.word(i - 1);
            if MODIFIERS.contains(&w) {
                i -= 1;
                continue;
            }
            if w == "@" {
                i -= 1;
                continue;
            }
            if w == ")" {
                let open = self.matching_open(i - 1);
                if open > 0 && self.word(open - 1) == "@" {
                    i = open - 1;
                    continue;
                }
                if open > 1 && self.ctx.code[open - 1].kind == Kind::Ident && self.word(open - 2) == "@" {
                    i = open - 2;
                    continue;
                }
            }
            break;
        }
        i
    }

    fn matching_open(&self, close: usize) -> usize {
        let mut depth = 0usize;
        let mut i = close;
        loop {
            match self.word(i) {
                ")" => depth += 1,
                "(" => {
                    depth -= 1;
                    if depth == 0 {
                        return i;
                    }
                }
                _ => {}
            }
            if i == 0 {
                return 0;
            }
            i -= 1;
        }
    }

    fn modifiers(&self, start: usize, end: usize) -> Vec<Span> {
        (start..end).filter(|&i| MODIFIERS.contains(&self.word(i))).map(|i| Span::of(&self.ctx.code[i])).collect()
    }

    /// The last Javadoc before `offset`, unless a statement or block boundary intervenes.
    fn javadoc_before(&self, offset: usize) -> Option<Span> {
        let k = self.docs.partition_point(|d| d.end <= offset);
        let doc = *self.docs.get(k.checked_sub(1)?)?;
        let lo = self.bounds.partition_point(|&b| b < doc.end);
        let hi = self.bounds.partition_point(|&b| b < offset);
        if hi > lo { None } else { Some(doc) }
    }

    /// The constant list, which ends at its `;`. Returns where the members begin.
    fn enum_constants(&mut self, start: usize, end: usize) -> usize {
        let mut i = start;
        while i < end {
            i = self.skip_annotations(i, end);
            if i >= end || self.ctx.code[i].kind != Kind::Ident {
                break;
            }
            let mut j = i + 1;
            if j < end && self.word(j) == "(" {
                j = self.skip_balanced(j, "(", ")");
            }
            if j < end && self.word(j) == "{" {
                j = self.pairs[j].map_or(j + 1, |c| c + 1);
            }
            // `enum E { void f() {} }` has no constants: a member follows the name.
            let next = self.ctx.code.get(j).map(|t| self.ctx.slice(t));
            if j < end && !matches!(next, Some(",") | Some(";")) {
                break;
            }
            let t = &self.ctx.code[i];
            let javadoc = self.javadoc_before(t.start).filter(|d| {
                let lo = self.commas.partition_point(|&c| c < d.end);
                let hi = self.commas.partition_point(|&c| c < t.start);
                hi == lo
            });
            self.out.enum_constants.push(EnumConstant { name: Span::of(t), javadoc });
            if next == Some(";") {
                return j + 1;
            }
            i = j + 1;
        }
        i.min(end)
    }

    fn skip_balanced(&self, open: usize, o: &str, c: &str) -> usize {
        let mut depth = 0usize;
        let mut i = open;
        while i < self.ctx.code.len() {
            let w = self.word(i);
            if w == o {
                depth += 1;
            } else if w == c {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            i += 1;
        }
        i
    }

    fn members(&mut self, start: usize, end: usize, in_interface: bool) {
        let mut i = start;
        while i < end {
            if let Some((kw, kind)) = self.nested_type(i, end) {
                i = self.type_decl(kw, kind).min(end);
                continue;
            }
            let (seg_end, next) = self.segment(i, end);
            if seg_end > i {
                self.member(i, seg_end, in_interface);
            }
            i = next.max(i + 1);
        }
    }

    /// A nested type usually carries modifiers: `private static class Foo`.
    fn nested_type(&self, i: usize, end: usize) -> Option<(usize, TypeKind)> {
        let mut j = self.skip_annotations(i, end);
        while j < end && MODIFIERS.contains(&self.word(j)) {
            j = self.skip_annotations(j + 1, end);
        }
        if j >= end {
            return None;
        }
        self.type_keyword(j).map(|kind| (j, kind))
    }

    /// End of one member: the first top-level `;`, or a block that is not an initializer.
    fn segment(&self, start: usize, end: usize) -> (usize, usize) {
        let mut depth = 0usize;
        let mut i = start;
        while i < end {
            match self.word(i) {
                "{" => {
                    if depth == 0 {
                        let close = self.pairs[i].unwrap_or(end.saturating_sub(1));
                        let after = self.ctx.code.get(close + 1).map(|t| self.ctx.slice(t));
                        if !matches!(after, Some(";") | Some(",") | Some(")")) {
                            return (close, close + 1);
                        }
                    }
                    depth += 1;
                }
                "}" => depth = depth.saturating_sub(1),
                ";" if depth == 0 => return (i, i + 1),
                _ => {}
            }
            i += 1;
        }
        (end, end)
    }

    fn member(&mut self, start: usize, end: usize, in_interface: bool) {
        let i = self.skip_annotations(start, end);
        if i >= end {
            return;
        }
        let mut paren = None;
        let mut depth = 0usize;
        for j in i..end {
            match self.word(j) {
                "(" if depth == 0 => {
                    paren = Some(j);
                    break;
                }
                // An initializer is not a signature: `Runnable r = new Runnable() {…};`
                "=" | "{" if depth == 0 => break,
                "{" => depth += 1,
                "}" => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        match paren {
            Some(p) => self.method(i, p, end, in_interface),
            None => self.fields(i, end, in_interface),
        }
    }

    fn skip_annotations(&self, mut i: usize, end: usize) -> usize {
        while i < end && self.word(i) == "@" {
            i += 1;
            if i < end && self.ctx.code[i].kind == Kind::Ident {
                i += 1;
            }
            while i + 1 < end && self.word(i) == "." {
                i += 2;
            }
            if i < end && self.word(i) == "(" {
                i = self.skip_balanced(i, "(", ")").min(end);
            }
        }
        i
    }

    fn method(&mut self, start: usize, paren: usize, end: usize, in_interface: bool) {
        let code = &self.ctx.code;
        if paren == 0 || code[paren - 1].kind != Kind::Ident {
            return;
        }
        let name = Span::of(&code[paren - 1]);
        let modifiers = self.modifiers(start, paren - 1);
        let close = self.skip_balanced(paren, "(", ")");
        let params = self.params(paren + 1, close.saturating_sub(1));
        let body = (close..end).find(|&j| self.word(j) == "{").map(|j| {
            let c = self.pairs[j].unwrap_or(end.saturating_sub(1));
            (j + 1, c)
        });
        let javadoc = self.javadoc_before(code[start].start);
        let constructor = self.type_names.last() == Some(&self.word(paren - 1));
        self.out.methods.push(MethodDecl { name, modifiers, params, body, javadoc, in_interface, constructor });
        if let Some((b, c)) = body {
            self.locals(b, c);
        }
    }

    fn params(&mut self, start: usize, end: usize) -> Vec<Param> {
        let mut out = Vec::new();
        let mut i = start;
        while i < end {
            i = self.skip_annotations(i, end);
            if i < end && self.word(i) == "final" {
                i += 1;
            }
            let (ty, next) = self.parse_type(i, end);
            if ty.is_empty() || next >= end || self.ctx.code[next].kind != Kind::Ident {
                break;
            }
            let name = Span::of(&self.ctx.code[next]);
            let array_after_name = next + 1 < end && self.word(next + 1) == "[";
            let mut j = next + 1;
            while j < end && self.word(j) != "," {
                j += 1;
            }
            out.push(Param { name, ty, array_after_name });
            i = j + 1;
        }
        out
    }

    /// One or more declarators sharing a type: `int a, b[];`.
    fn fields(&mut self, start: usize, end: usize, in_interface: bool) {
        let mut m = start;
        while m < end && MODIFIERS.contains(&self.word(m)) {
            m += 1;
        }
        let (ty, mut i) = self.parse_type(m, end);
        if ty.is_empty() {
            return;
        }
        let modifiers = self.modifiers(start, m);
        loop {
            if i >= end || self.ctx.code[i].kind != Kind::Ident {
                return;
            }
            let name = Span::of(&self.ctx.code[i]);
            let array_after_name = i + 1 < end && self.word(i + 1) == "[";
            self.out.fields.push(FieldDecl {
                name,
                modifiers: modifiers.clone(),
                ty: ty.clone(),
                array_after_name,
                in_interface,
            });
            i += 1;
            while i < end && !matches!(self.word(i), "," | "=" | ";") {
                i += 1;
            }
            if i >= end || self.word(i) != "," {
                return;
            }
            i += 1;
        }
    }

    /// A type: qualified name, generic arguments, array brackets, varargs.
    fn parse_type(&self, start: usize, end: usize) -> (String, usize) {
        let code = &self.ctx.code;
        let mut i = start;
        if i >= end || code[i].kind != Kind::Ident || NON_TYPE.contains(&self.word(i)) {
            return (String::new(), start);
        }
        let from = code[i].start;
        i += 1;
        while i + 1 < end && self.word(i) == "." && code[i + 1].kind == Kind::Ident {
            i += 2;
        }
        if i < end && self.word(i) == "<" {
            let mut depth = 0usize;
            while i < end {
                match self.word(i) {
                    "<" => depth += 1,
                    ">" => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        }
        let mut to = code[i - 1].end;
        while i + 1 < end && self.word(i) == "[" && self.word(i + 1) == "]" {
            to = code[i + 1].end;
            i += 2;
        }
        if i < end && self.word(i) == "..." {
            to = code[i].end;
            i += 1;
        }
        (self.ctx.text[from..to].to_string(), i)
    }

    /// Local declarations in a body, recursing into nested blocks and loop headers.
    fn locals(&mut self, start: usize, end: usize) {
        let mut i = start;
        while i < end {
            let w = self.word(i);
            if w == "{" {
                let close = self.pairs[i].unwrap_or(end.saturating_sub(1));
                self.locals(i + 1, close);
                i = close + 1;
                continue;
            }
            if w == "for" && i + 1 < end && self.word(i + 1) == "(" {
                let close = self.skip_balanced(i + 1, "(", ")");
                self.for_locals(i + 2, close.saturating_sub(1));
                i = close;
                continue;
            }
            let stmt_end = self.statement_end(i, end);
            if stmt_end > i {
                self.local_at(i, stmt_end);
            }
            i = stmt_end.max(i + 1);
        }
    }

    /// Stops before a block so the caller can recurse into it.
    fn statement_end(&self, start: usize, end: usize) -> usize {
        let mut depth = 0usize;
        let mut i = start;
        while i < end {
            match self.word(i) {
                "(" | "[" => depth += 1,
                ")" | "]" => depth = depth.saturating_sub(1),
                "{" => return i,
                ";" if depth == 0 => return i + 1,
                _ => {}
            }
            i += 1;
        }
        end
    }

    fn local_at(&mut self, start: usize, end: usize) {
        let mut i = self.skip_annotations(start, end);
        if i < end && self.word(i) == "final" {
            i += 1;
        }
        let (ty, j) = self.parse_type(i, end);
        if ty.is_empty() || j >= end || self.ctx.code[j].kind != Kind::Ident {
            return;
        }
        let after = self.ctx.code.get(j + 1).map(|t| self.ctx.slice(t));
        if !matches!(after, Some("=") | Some(";") | Some(",") | Some("[") | None) {
            return;
        }
        self.out.locals.push(LocalDecl { name: Span::of(&self.ctx.code[j]), ty, array_after_name: after == Some("[") });
    }

    fn for_locals(&mut self, start: usize, end: usize) {
        let (ty, i) = self.parse_type(start, end);
        if ty.is_empty() || i >= end || self.ctx.code[i].kind != Kind::Ident {
            return;
        }
        if matches!(self.ctx.code.get(i + 1).map(|t| self.ctx.slice(t)), Some(":") | Some("=")) {
            self.out.locals.push(LocalDecl { name: Span::of(&self.ctx.code[i]), ty, array_after_name: false });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_names(ctx: &Ctx, d: &Decl) -> Vec<String> {
        d.fields.iter().map(|f| f.name.text(ctx).to_string()).collect()
    }

    fn method_names(ctx: &Ctx, d: &Decl) -> Vec<String> {
        d.methods.iter().map(|m| m.name.text(ctx).to_string()).collect()
    }

    fn local_names(ctx: &Ctx, d: &Decl) -> Vec<String> {
        d.locals.iter().map(|l| l.name.text(ctx).to_string()).collect()
    }

    #[test]
    fn reads_the_package_and_type_kind() {
        let src = "package com.alibaba.open.util; public class A {}";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(d.package.unwrap().text(&ctx), "com.alibaba.open.util");
        assert_eq!(d.types.len(), 1);
        assert_eq!(d.types[0].kind, TypeKind::Class);
        assert_eq!(d.types[0].name.text(&ctx), "A");
        assert!(has_modifier(&ctx, &d.types[0].modifiers, "public"));
    }

    #[test]
    fn fields_keep_modifiers_type_and_declarators() {
        let src = r#"class A { private static final String NAME = "x"; int a, b[]; java.util.List<String> c; }"#;
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(field_names(&ctx, &d), ["NAME", "a", "b", "c"]);
        assert_eq!(d.fields[0].ty, "String");
        assert!(has_modifier(&ctx, &d.fields[0].modifiers, "static"));
        assert!(has_modifier(&ctx, &d.fields[0].modifiers, "final"));
        assert!(has_modifier(&ctx, &d.fields[0].modifiers, "private"));
        assert!(!d.fields[1].array_after_name);
        assert!(d.fields[2].array_after_name);
        assert_eq!(d.fields[3].ty, "java.util.List<String>");
    }

    #[test]
    fn methods_keep_params_modifiers_and_body() {
        let src = "class A { public static void f(int x, String[] y, final int z) { int w = 1; } }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(method_names(&ctx, &d), ["f"]);
        let m = &d.methods[0];
        assert!(has_modifier(&ctx, &m.modifiers, "static"));
        assert_eq!(m.params.iter().map(|p| p.name.text(&ctx)).collect::<Vec<_>>(), ["x", "y", "z"]);
        assert_eq!(m.params[1].ty, "String[]");
        assert!(!m.params[1].array_after_name);
        assert!(m.body.is_some());
        assert_eq!(local_names(&ctx, &d), ["w"]);
    }

    #[test]
    fn c_style_array_parameters_are_marked() {
        let src = "class A { void f(String args[]) {} }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert!(d.methods[0].params[0].array_after_name);
        assert_eq!(d.methods[0].params[0].ty, "String");
    }

    #[test]
    fn locals_include_nested_blocks_and_loop_variables() {
        let src = "class A { void f() { if (ok) { int a = 1; } for (String s : list) { int b = 2; } \
                   for (int i = 0; i < 9; i++) { int c = 3; } int d = 4; } }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(local_names(&ctx, &d), ["a", "s", "b", "i", "c", "d"]);
    }

    #[test]
    fn calls_and_assignments_are_not_declarations() {
        let src = "class A { void f() { foo.bar(); x = 5; new Foo(); return; this.x = 1; } }";
        let ctx = Ctx::new(src);
        assert!(parse(&ctx).locals.is_empty());
    }

    #[test]
    fn javadoc_attaches_to_the_following_declaration_only() {
        let src = "class A { /** f */ void f() {} void g() {} } enum E { /** a */ A, B }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert!(d.methods[0].javadoc.is_some());
        assert!(d.methods[1].javadoc.is_none(), "g must not inherit f's Javadoc");
        assert!(d.enum_constants[0].javadoc.is_some());
        assert!(d.enum_constants[1].javadoc.is_none(), "B must not inherit A's Javadoc");
    }

    #[test]
    fn annotations_do_not_steal_the_javadoc() {
        let src = "class A { /** doc */ @Override @SuppressWarnings(\"x\") public void f() {} }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert!(d.methods[0].javadoc.is_some());
        assert!(has_modifier(&ctx, &d.methods[0].modifiers, "public"));
    }

    #[test]
    fn interface_methods_have_no_body() {
        let src = "interface I { void f(); int g(int x); }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(d.types[0].kind, TypeKind::Interface);
        assert_eq!(method_names(&ctx, &d), ["f", "g"]);
        assert!(d.methods.iter().all(|m| m.body.is_none() && m.in_interface));
    }

    #[test]
    fn enum_constants_are_collected_with_their_javadoc() {
        let src = "enum E { /** a */ A, B(1), C; void f() {} }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(d.types[0].kind, TypeKind::Enum);
        assert_eq!(d.enum_constants.iter().map(|c| c.name.text(&ctx)).collect::<Vec<_>>(), ["A", "B", "C"]);
        assert!(d.enum_constants[0].javadoc.is_some());
        assert!(d.enum_constants[1].javadoc.is_none());
        assert_eq!(method_names(&ctx, &d), ["f"]);
    }

    #[test]
    fn record_components_become_fields() {
        let src = "record R(int x, String[] y) {}";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(d.types[0].kind, TypeKind::Record);
        assert_eq!(field_names(&ctx, &d), ["x", "y"]);
        assert_eq!(d.fields[1].ty, "String[]");
    }

    #[test]
    fn nested_types_are_parsed() {
        let src = "class A { int outer; class B { int inner; void g() {} } }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(d.types.iter().map(|t| t.name.text(&ctx)).collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(field_names(&ctx, &d), ["outer", "inner"]);
        assert_eq!(method_names(&ctx, &d), ["g"]);
    }

    #[test]
    fn a_class_literal_is_not_a_declaration() {
        let src = "class A { void f() { Class<?> c = String.class; } }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(d.types.len(), 1);
        assert_eq!(local_names(&ctx, &d), ["c"]);
    }

    #[test]
    fn superclass_is_captured_for_the_exception_rule() {
        let src = "class MyError extends RuntimeException {} class Ok extends Base {}";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(d.types[0].superclass.as_deref(), Some("RuntimeException"));
        assert_eq!(d.types[1].superclass.as_deref(), Some("Base"));
    }

    #[test]
    fn abstract_and_generic_declarations_parse() {
        let src = "public abstract class AbstractA<T extends Comparable<T>> { protected abstract T f(T x); }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert!(has_modifier(&ctx, &d.types[0].modifiers, "abstract"));
        assert_eq!(d.types[0].name.text(&ctx), "AbstractA");
        assert_eq!(method_names(&ctx, &d), ["f"]);
        assert!(d.methods[0].body.is_none());
    }

    #[test]
    fn initializers_and_anonymous_classes_stay_fields() {
        let src =
            "class A { int[] a = {1, 2}; Runnable r = new Runnable() { public void run() {} }; static { init(); } }";
        let ctx = Ctx::new(src);
        let d = parse(&ctx);
        assert_eq!(field_names(&ctx, &d), ["a", "r"]);
        assert_eq!(d.fields[0].ty, "int[]");
    }
}
