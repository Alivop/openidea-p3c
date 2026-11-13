//! 注释规约（手册·注释规约）。两条规则默认关闭，需在设置里打开：
//! 老项目补齐 Javadoc 之前，开着只会刷屏。

use super::Ctx;
use super::decl::has_modifier;
use openidea_plugin::{Diagnostic, Severity};

/// 手册·注释规约 1：所有的抽象方法（包括接口中的方法）必须用 Javadoc 注释。
pub fn abstract_method_javadoc(ctx: &Ctx) -> Vec<Diagnostic> {
    if !ctx.settings().abstract_method_javadoc {
        return Vec::new();
    }
    let mut out = Vec::new();
    for m in &ctx.decl().methods {
        if !(m.in_interface || has_modifier(ctx, &m.modifiers, "abstract")) || m.javadoc.is_some() {
            continue;
        }
        out.push(ctx.diag(
            m.name.start,
            m.name.end,
            Severity::Warning,
            ctx.t(
                "抽象方法与接口方法必须用 Javadoc 注释：除返回值、参数、异常说明外，\
                 还要指出该方法做什么事情、实现什么功能（手册·注释规约 1）",
                "Abstract and interface methods must have Javadoc: besides the return value, parameters and \
                 exceptions, state what the method does (Handbook: Comments 1)",
            ),
        ));
    }
    out
}

/// 手册·注释规约 5：所有的枚举类型字段必须要有注释，说明每个数据项的用途。
pub fn enum_constant_javadoc(ctx: &Ctx) -> Vec<Diagnostic> {
    if !ctx.settings().enum_constant_javadoc {
        return Vec::new();
    }
    let mut out = Vec::new();
    for c in &ctx.decl().enum_constants {
        if c.javadoc.is_some() {
            continue;
        }
        out.push(ctx.diag(
            c.name.start,
            c.name.end,
            Severity::Warning,
            ctx.t(
                "枚举常量必须有注释，说明每个数据项的用途（手册·注释规约 5）",
                "Enum constants must have a comment explaining what each item is for (Handbook: Comments 5)",
            ),
        ));
    }
    out
}

/// 手册·注释规约：所有的类都应有 @author。默认关闭，需在设置里打开。
pub fn class_author(ctx: &Ctx) -> Vec<Diagnostic> {
    if !ctx.settings().class_must_have_author {
        return Vec::new();
    }
    let mut out = Vec::new();
    for t in &ctx.decl().types {
        if t.javadoc.is_some_and(|doc| doc.text(ctx).contains("@author")) {
            continue;
        }
        out.push(ctx.diag(
            t.name.start,
            t.name.end,
            Severity::Warning,
            ctx.t(
                "所有的类（含接口、枚举）都应在 Javadoc 中标注 @author，写明创建者，便于追溯（手册·注释规约）",
                "Every class (including interfaces and enums) should carry an @author tag in its Javadoc naming \
                 its creator, for traceability (Handbook: Comments)",
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn with_setting(key: &str, rule: fn(&Ctx) -> Vec<Diagnostic>, src: &str) -> Vec<Diagnostic> {
        let mock = MockHost::install();
        mock.set_setting(key, "true");
        rule(&Ctx::new(src))
    }

    #[test]
    fn both_rules_are_off_by_default() {
        let _mock = MockHost::install();
        let src = "interface I { void f(); } enum E { A, B }";
        assert!(abstract_method_javadoc(&Ctx::new(src)).is_empty());
        assert!(enum_constant_javadoc(&Ctx::new(src)).is_empty());
    }

    #[test]
    fn interface_and_abstract_methods_need_javadoc() {
        let src = "interface I { void f(); } abstract class A { abstract void g(); }";
        assert_eq!(with_setting("comment.abstractMethodJavadoc", abstract_method_javadoc, src).len(), 2);
        let src = "interface I { /** 做事 */ void f(); } abstract class A { /** 做事 */ abstract void g(); }";
        assert!(with_setting("comment.abstractMethodJavadoc", abstract_method_javadoc, src).is_empty());
    }

    #[test]
    fn concrete_methods_are_not_required_to_have_javadoc() {
        let src = "class A { public void f() {} private void g() {} }";
        assert!(with_setting("comment.abstractMethodJavadoc", abstract_method_javadoc, src).is_empty());
        let src = "abstract class A { void f() {} }";
        assert!(with_setting("comment.abstractMethodJavadoc", abstract_method_javadoc, src).is_empty());
    }

    #[test]
    fn enum_constants_need_their_own_javadoc() {
        let src = "enum E { /** 甲 */ A, B, /** 丙 */ C }";
        let got = with_setting("comment.enumConstantJavadoc", enum_constant_javadoc, src);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].range.start.character, 21);
        assert!(
            with_setting("comment.enumConstantJavadoc", enum_constant_javadoc, "enum E { /** 甲 */ A, /** 乙 */ B }")
                .is_empty()
        );
    }

    #[test]
    fn an_empty_enum_and_literals_are_left_alone() {
        assert!(with_setting("comment.enumConstantJavadoc", enum_constant_javadoc, "enum E { }").is_empty());
        assert!(
            with_setting("comment.enumConstantJavadoc", enum_constant_javadoc, "class A { String s = \"A, B\"; }")
                .is_empty()
        );
        assert!(
            with_setting(
                "comment.abstractMethodJavadoc",
                abstract_method_javadoc,
                "class A { String s = \"void f();\"; }"
            )
            .is_empty()
        );
    }

    #[test]
    fn class_author_is_off_by_default() {
        let _mock = MockHost::install();
        assert!(class_author(&Ctx::new("class A {}")).is_empty());
    }

    #[test]
    fn classes_need_an_author_tag() {
        assert_eq!(with_setting("comment.classMustHaveAuthor", class_author, "class A {}").len(), 1);
        assert_eq!(with_setting("comment.classMustHaveAuthor", class_author, "/** demo */ class A {}").len(), 1);
        assert_eq!(with_setting("comment.classMustHaveAuthor", class_author, "interface I {} enum E { X }").len(), 2);
    }

    #[test]
    fn an_author_tag_satisfies_the_rule() {
        assert!(
            with_setting("comment.classMustHaveAuthor", class_author, "/** @author kerie */ class A {}").is_empty()
        );
    }
}
