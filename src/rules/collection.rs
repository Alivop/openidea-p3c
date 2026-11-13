//! 集合处理

use super::{Ctx, seq, skip_generics};
use crate::lex::Kind;
use openidea_plugin::{Diagnostic, Severity};

/// 手册·集合处理 3：无参 toArray() 返回 Object[]，强转其它数组类型会抛 ClassCastException。
pub fn to_array(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &[".", "toArray", "(", ")"]) {
            continue;
        }
        let t = &ctx.code[i + 1];
        out.push(ctx.diag(
            t.start,
            t.end,
            Severity::Warning,
            ctx.t(
                "集合转数组应使用 toArray(T[] array)，传入类型一致、长度为 list.size() 的数组；无参 toArray() \
                 返回 Object[]，强转成其它数组类型会抛 ClassCastException（手册·集合处理 3）",
                "Convert a collection with toArray(T[] array), passing an array of the same type and length \
                 list.size(); the no-arg toArray() returns Object[], and casting it to another array type throws \
                 ClassCastException (Handbook: Collections 3)",
            ),
        ));
    }
    out
}

/// 手册·集合处理 5：Arrays.asList 返回固定大小的视图，add/remove/clear 会抛异常。
pub fn arrays_aslist_mutation(ctx: &Ctx) -> Vec<Diagnostic> {
    const MUTATORS: [&str; 6] = ["add", "remove", "clear", "addAll", "removeAll", "retainAll"];
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["Arrays", ".", "asList", "("]) {
            continue;
        }
        let Some(close) = ctx.parens()[i + 3] else { continue };
        let report = |at: usize, out: &mut Vec<Diagnostic>| {
            let Some(m) = ctx.code.get(at + 2) else { return };
            if seq(ctx, at + 1, &["."]) && MUTATORS.contains(&ctx.slice(m)) && seq(ctx, at + 3, &["("]) {
                let method = ctx.slice(m);
                let message = if ctx.zh() {
                    format!(
                        "Arrays.asList 返回的是 Arrays 内部的固定大小视图，调用 {method} 会抛 \
                         UnsupportedOperationException；需要增删请改用 new ArrayList<>(Arrays.asList(…))（手册·集合处理 5）"
                    )
                } else {
                    format!(
                        "Arrays.asList returns a fixed-size view backed by Arrays, so calling {method} throws \
                         UnsupportedOperationException; to add or remove elements use new ArrayList<>(Arrays.asList(…)) \
                         (Handbook: Collections 5)"
                    )
                };
                out.push(ctx.diag(m.start, m.end, Severity::Error, &message));
            }
        };
        // 直接对返回值动手：`Arrays.asList(a, b).add(c)`。
        report(close, &mut out);

        if i < 2 || !seq(ctx, i - 1, &["="]) || ctx.code[i - 2].kind != Kind::Ident {
            continue;
        }
        let name = ctx.slice(&ctx.code[i - 2]);
        // 只在声明所在的块内查找，避免同名的其它变量误报。
        let end = ctx
            .braces()
            .iter()
            .enumerate()
            .filter(|&(o, c)| o < i && c.is_some_and(|c| c > i))
            .map(|(o, _)| o)
            .max()
            .and_then(|o| ctx.braces()[o])
            .unwrap_or(ctx.code.len());
        for j in (close + 1)..end {
            if ctx.slice(&ctx.code[j]) == name {
                report(j, &mut out);
            }
        }
    }
    out
}

/// 手册·集合处理：哈希类集合初始化时指定初始容量，避免反复扩容重建。默认关闭，需在设置里打开。
const CAPACITY_TYPES: [&str; 6] =
    ["HashMap", "LinkedHashMap", "Hashtable", "HashSet", "LinkedHashSet", "ConcurrentHashMap"];

pub fn init_capacity(ctx: &Ctx) -> Vec<Diagnostic> {
    if !ctx.settings().collection_init_capacity {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["new"]) || !ctx.code.get(i + 1).is_some_and(|t| t.kind == Kind::Ident) {
            continue;
        }
        let mut k = i + 1;
        while seq(ctx, k + 1, &["."]) && ctx.code.get(k + 2).is_some_and(|t| t.kind == Kind::Ident) {
            k += 2;
        }
        if !CAPACITY_TYPES.contains(&ctx.slice(&ctx.code[k])) {
            continue;
        }
        let mut p = k + 1;
        if seq(ctx, p, &["<"]) {
            p = skip_generics(ctx, p);
        }
        // 只有无参构造才需要提醒；给了容量或拷贝来源的放过。
        if !seq(ctx, p, &["(", ")"]) {
            continue;
        }
        out.push(ctx.diag(
            ctx.code[k].start,
            ctx.code[k].end,
            Severity::Warning,
            ctx.t(
                "集合初始化时应指定初始容量，避免元素增多时反复扩容重建哈希表；如 new HashMap<>(16)，\
                 容量可按 预期元素数 / 0.75 + 1 估算（手册·集合处理）",
                "Specify an initial capacity when creating the collection to avoid repeated resizing and rehashing \
                 as it grows, e.g. new HashMap<>(16); estimate capacity as expectedSize / 0.75 + 1 \
                 (Handbook: Collections)",
            ),
        ));
    }
    out
}

/// 手册·集合处理：subList 返回的是视图，强转 ArrayList/LinkedList 会抛 ClassCastException；
/// 需要独立列表用 new ArrayList<>(list.subList(...))。
pub fn sublist_cast(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["("])
            || !ctx.code.get(i + 1).is_some_and(|t| matches!(ctx.slice(t), "ArrayList" | "LinkedList"))
        {
            continue;
        }
        let mut j = i + 2;
        if seq(ctx, j, &["<"]) {
            j = skip_generics(ctx, j);
        }
        if !seq(ctx, j, &[")"]) {
            continue;
        }
        let r = j + 1;
        if !ctx.code.get(r).is_some_and(|t| t.kind == Kind::Ident) || !seq(ctx, r + 1, &[".", "subList", "("]) {
            continue;
        }
        let m = &ctx.code[r + 2];
        out.push(ctx.diag(
            ctx.code[i].start,
            m.end,
            Severity::Warning,
            ctx.t(
                "subList 返回的是原列表的视图（内部类），强转成 ArrayList 会抛 ClassCastException；\
                 需要独立列表请用 new ArrayList<>(list.subList(from, to))（手册·集合处理）",
                "subList returns a view backed by the original list (an inner class), so casting it to ArrayList \
                 throws ClassCastException; for an independent list use new ArrayList<>(list.subList(from, to)) \
                 (Handbook: Collections)",
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn hits(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        to_array(&Ctx::new(src))
    }

    #[test]
    fn flags_the_no_argument_form_only() {
        assert_eq!(hits("String[] a = list.toArray();").len(), 1);
        assert_eq!(hits("String[] a = list.toArray(new String[0]);").len(), 0);
        assert_eq!(hits("String[] a = list.toArray(new String[list.size()]);").len(), 0);
    }

    #[test]
    fn ignores_strings_and_comments() {
        assert_eq!(hits(r#"String s = "list.toArray()"; // list.toArray()"#).len(), 0);
    }

    #[test]
    fn the_range_covers_the_method_name() {
        let src = "Object[] a = list.toArray();";
        let d = &hits(src)[0];
        assert_eq!(&src[d.range.start.character as usize..d.range.end.character as usize], "toArray");
    }

    #[test]
    fn every_occurrence_is_reported() {
        assert_eq!(hits("a.toArray(); b.toArray();").len(), 2);
    }

    fn mutations(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        arrays_aslist_mutation(&Ctx::new(src))
    }

    #[test]
    fn mutating_the_assigned_variable_is_flagged() {
        let src = "class A { void f() { List<String> l = Arrays.asList(\"a\"); l.add(\"b\"); } }";
        let got = mutations(src);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].severity, Severity::Error);
        assert_eq!(
            mutations("class A { void f() { List<String> l = Arrays.asList(\"a\"); l.remove(0); l.clear(); } }").len(),
            2
        );
    }

    #[test]
    fn mutating_the_result_directly_is_flagged() {
        assert_eq!(mutations("class A { void f() { Arrays.asList(\"a\").add(\"b\"); } }").len(), 1);
        assert_eq!(mutations("class A { void f() { Arrays.asList(\"a\").clear(); } }").len(), 1);
    }

    #[test]
    fn reads_and_set_are_allowed_on_the_view() {
        assert_eq!(
            mutations("class A { void f() { List<String> l = Arrays.asList(\"a\"); l.get(0); l.set(0, \"b\"); } }")
                .len(),
            0
        );
        assert_eq!(
            mutations("class A { void f() { List<String> l = new ArrayList<>(Arrays.asList(\"a\")); l.add(\"b\"); } }")
                .len(),
            0
        );
        assert_eq!(mutations("class A { void f() { return Arrays.asList(\"a\"); } }").len(), 0);
    }

    #[test]
    fn a_same_named_variable_in_another_block_is_not_flagged() {
        let src = "class A { void f() { { List<String> l = Arrays.asList(\"a\"); } { List<String> l = new ArrayList<>(); l.add(\"b\"); } } }";
        assert_eq!(mutations(src).len(), 0);
    }

    fn capacity(src: &str) -> Vec<Diagnostic> {
        let mock = MockHost::install();
        mock.set_setting("collection.initCapacity", "true");
        init_capacity(&Ctx::new(src))
    }

    #[test]
    fn init_capacity_is_off_by_default() {
        let _mock = MockHost::install();
        assert!(init_capacity(&Ctx::new("Map m = new HashMap<>();")).is_empty());
    }

    #[test]
    fn flags_no_arg_hash_collections() {
        assert_eq!(capacity("Map m = new HashMap<>();").len(), 1);
        assert_eq!(capacity("Map m = new HashMap<String, String>();").len(), 1);
        assert_eq!(capacity("Set s = new HashSet<>();").len(), 1);
        assert_eq!(capacity("Map m = new java.util.HashMap<>();").len(), 1);
        assert_eq!(capacity("Map m = new ConcurrentHashMap<>();").len(), 1);
    }

    #[test]
    fn capacity_given_and_other_collections_are_fine() {
        assert_eq!(capacity("Map m = new HashMap<>(16);").len(), 0);
        assert_eq!(capacity("Map m = new HashMap<String, String>(16);").len(), 0);
        assert_eq!(capacity("List l = new ArrayList<>();").len(), 0);
        assert_eq!(capacity("Map m = new TreeMap<>();").len(), 0);
        assert_eq!(capacity("String s = \"new HashMap<>()\";").len(), 0);
    }

    fn sublist(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        sublist_cast(&Ctx::new(src))
    }

    #[test]
    fn flags_casting_a_sublist_view() {
        assert_eq!(sublist("List t = (ArrayList<String>) list.subList(0, 1);").len(), 1);
        assert_eq!(sublist("List t = (ArrayList) list.subList(0, 1);").len(), 1);
        assert_eq!(sublist("List t = (LinkedList<String>) list.subList(0, 1);").len(), 1);
    }

    #[test]
    fn copying_the_sublist_and_plain_casts_are_fine() {
        assert_eq!(sublist("List t = new ArrayList<>(list.subList(0, 1));").len(), 0);
        assert_eq!(sublist("ArrayList a = (ArrayList) other;").len(), 0);
        assert_eq!(sublist("String s = \"(ArrayList) list.subList(0,1)\";").len(), 0);
    }
}
