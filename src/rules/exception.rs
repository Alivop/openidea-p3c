//! 异常日志（手册·异常日志）。

use super::{Ctx, annotation_name, seq};
use openidea_plugin::{Diagnostic, Severity};

/// 手册·异常日志：@Transactional 应指定 rollbackFor。默认只回滚 RuntimeException/Error，
/// 受检异常不会触发回滚。无法做数据流分析，故对编程式 rollback 与只读事务保守放过。
pub fn transactional_rollback(ctx: &Ctx) -> Vec<Diagnostic> {
    // 正例 3 靠 transactionManager.rollback 手动回滚，按文件粒度保守跳过。
    if (0..ctx.code.len()).any(|i| seq(ctx, i, &[".", "rollback", "("])) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if ctx.slice(&ctx.code[i]) != "@" {
            continue;
        }
        let Some((name, next)) = annotation_name(ctx, i) else { continue };
        if name != "Transactional" {
            continue;
        }
        let exempt = if seq(ctx, next, &["("]) {
            let Some(close) = ctx.parens()[next] else { continue };
            let args = &ctx.code[next + 1..close];
            let has_rollback_for =
                args.iter().any(|t| matches!(ctx.slice(t), "rollbackFor" | "rollbackForClassName"));
            let read_only = args
                .windows(3)
                .any(|w| ctx.slice(&w[0]) == "readOnly" && ctx.slice(&w[1]) == "=" && ctx.slice(&w[2]) == "true");
            has_rollback_for || read_only
        } else {
            false
        };
        if exempt {
            continue;
        }
        let name_tok = &ctx.code[next - 1];
        out.push(ctx.diag(
            name_tok.start,
            name_tok.end,
            Severity::Warning,
            ctx.t(
                "@Transactional 应指定 rollbackFor（如 rollbackFor = Exception.class）：默认只回滚 \
                 RuntimeException 和 Error，抛出的受检异常不会触发回滚（手册·异常日志）",
                "@Transactional should specify rollbackFor (e.g. rollbackFor = Exception.class): by default only \
                 RuntimeException and Error roll back, so a thrown checked exception does not trigger a rollback \
                 (Handbook: Exceptions & Logging)",
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
        transactional_rollback(&Ctx::new(src))
    }

    #[test]
    fn transactional_without_rollback_for_is_flagged() {
        assert_eq!(hits("class A { @Transactional public void save() {} }").len(), 1);
        assert_eq!(hits("@Transactional class A {}").len(), 1);
        assert_eq!(
            hits("class A { @Transactional(propagation = Propagation.REQUIRED) public void save() {} }").len(),
            1
        );
        assert_eq!(hits("class A { @org.springframework.transaction.annotation.Transactional void save() {} }").len(), 1);
    }

    #[test]
    fn rollback_for_and_read_only_satisfy_the_rule() {
        assert_eq!(hits("class A { @Transactional(rollbackFor = Exception.class) public void save() {} }").len(), 0);
        assert_eq!(
            hits("class A { @Transactional(rollbackForClassName = \"Exception\") public void save() {} }").len(),
            0
        );
        assert_eq!(hits("class A { @Transactional(readOnly = true) public void load() {} }").len(), 0);
    }

    #[test]
    fn programmatic_rollback_is_left_alone() {
        let src = "class A { @Transactional public void save() { transactionManager.rollback(status); } }";
        assert_eq!(hits(src).len(), 0);
    }

    #[test]
    fn other_annotations_and_strings_are_ignored() {
        assert_eq!(hits("class A { @Override public void save() {} }").len(), 0);
        assert_eq!(hits("class A { String s = \"@Transactional\"; }").len(), 0);
    }
}