//! 并发处理

use super::{Ctx, seq};
use openidea_plugin::{Diagnostic, Severity};

/// 手册·并发处理 4：这些工厂的队列长度或线程数上限是 Integer.MAX_VALUE。
const BANNED: [&str; 5] = [
    "newFixedThreadPool",
    "newSingleThreadExecutor",
    "newCachedThreadPool",
    "newScheduledThreadPool",
    "newSingleThreadScheduledExecutor",
];

pub fn executors(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["Executors", "."]) {
            continue;
        }
        let Some(name) = ctx.code.get(i + 2) else {
            continue;
        };
        let text = ctx.slice(name);
        if !BANNED.contains(&text) || !seq(ctx, i + 3, &["("]) {
            continue;
        }
        let message = if ctx.zh() {
            format!(
                "线程池应由 ThreadPoolExecutor 创建：Executors.{text} 的队列长度或线程数上限为 \
                 Integer.MAX_VALUE，请求堆积会耗尽资源导致 OOM（手册·并发处理 4）"
            )
        } else {
            format!(
                "Create thread pools with ThreadPoolExecutor: Executors.{text} bounds its queue length or thread \
                 count by Integer.MAX_VALUE, so a backlog of requests exhausts resources and causes OOM \
                 (Handbook: Concurrency 4)"
            )
        };
        out.push(ctx.diag(name.start, name.end, Severity::Warning, &message));
    }
    out
}

/// 手册·并发处理 3：线程应由线程池提供，不允许显式创建。
pub fn new_thread(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        if !seq(ctx, i, &["new", "Thread", "("]) {
            continue;
        }
        let t = &ctx.code[i + 1];
        out.push(ctx.diag(
            t.start,
            t.end,
            Severity::Warning,
            ctx.t(
                "线程应通过线程池提供，显式 new Thread 会频繁创建销毁线程，可能导致资源耗尽或过度切换（手册·并发处理 3）",
                "Threads should come from a thread pool: an explicit new Thread creates and destroys threads \
                 repeatedly, which can exhaust resources or cause excessive context switching \
                 (Handbook: Concurrency 3)",
            ),
        ));
    }
    out
}

/// 手册·并发处理 6：Timer 未捕获异常会终止所有定时线程。
pub fn timer(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for i in 0..ctx.code.len() {
        let at = if seq(ctx, i, &["new", "Timer", "("]) {
            i + 1
        } else if seq(ctx, i, &["new", "java", ".", "util", ".", "Timer", "("]) {
            i + 5
        } else {
            continue;
        };
        let t = &ctx.code[at];
        out.push(ctx.diag(
            t.start,
            t.end,
            Severity::Warning,
            ctx.t(
                "多个定时任务应使用 ScheduledExecutorService：Timer 在未捕获异常时会终止所有运行中的线程（手册·并发处理 6）",
                "Use ScheduledExecutorService for multiple scheduled tasks: an uncaught exception in a Timer \
                 terminates every running timer thread (Handbook: Concurrency 6)",
            ),
        ));
    }
    out
}

/// 手册·并发处理 6：SimpleDateFormat 线程不安全，不要定义为 static 变量。
pub fn static_date_format(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for f in &ctx.decl().fields {
        if !super::decl::has_modifier(ctx, &f.modifiers, "static") {
            continue;
        }
        let ty = f.ty.rsplit('.').next().unwrap_or(&f.ty);
        if !matches!(ty, "SimpleDateFormat" | "DateFormat") {
            continue;
        }
        out.push(ctx.diag(
            f.name.start,
            f.name.end,
            Severity::Error,
            ctx.t(
                "SimpleDateFormat 是线程不安全的类，不要定义为 static 变量：多线程共用会解析出错误的日期，\
                 甚至抛 NumberFormatException。请改用线程安全的 DateTimeFormatter，\
                 或每次新建实例 / 加锁（手册·并发处理 6）",
                "SimpleDateFormat is not thread-safe, so do not declare it static: sharing it across threads \
                 parses wrong dates and can even throw NumberFormatException. Use the thread-safe \
                 DateTimeFormatter, or create an instance per use / guard it with a lock (Handbook: Concurrency 6)",
            ),
        ));
    }
    out
}

/// 手册·并发处理：Random 实例被多线程共享会因竞争同一 seed 而性能下降，
/// 不要定义为 static 字段；改用 ThreadLocalRandom。
pub fn static_random(ctx: &Ctx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for f in &ctx.decl().fields {
        if !super::decl::has_modifier(ctx, &f.modifiers, "static") {
            continue;
        }
        let ty = f.ty.rsplit('.').next().unwrap_or(&f.ty);
        if ty != "Random" {
            continue;
        }
        out.push(ctx.diag(
            f.name.start,
            f.name.end,
            Severity::Warning,
            ctx.t(
                "Random 实例被多线程共享时会因竞争同一 seed 导致性能下降，不要定义为 static 字段；\
                 请改用 ThreadLocalRandom.current()（手册·并发处理）",
                "A Random instance shared across threads contends on the same seed and slows down; do not declare \
                 it static. Use ThreadLocalRandom.current() instead (Handbook: Concurrency)",
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
        let ctx = Ctx::new(src);
        let mut out = executors(&ctx);
        out.extend(new_thread(&ctx));
        out.extend(timer(&ctx));
        out
    }

    #[test]
    fn flags_each_banned_factory() {
        for name in BANNED {
            let src = format!("ExecutorService e = Executors.{name}(4);");
            assert_eq!(hits(&src).len(), 1, "{name}");
        }
    }

    #[test]
    fn leaves_thread_pool_executor_and_unrelated_calls_alone() {
        assert_eq!(hits("new ThreadPoolExecutor(1, 2, 3L, TimeUnit.SECONDS, new LinkedBlockingQueue<>(10));").len(), 0);
        assert_eq!(hits("Executors.defaultThreadFactory();").len(), 0);
        assert_eq!(hits("String s = \"Executors.newFixedThreadPool(4)\";").len(), 0);
    }

    #[test]
    fn a_method_reference_without_a_call_is_not_flagged() {
        assert_eq!(hits("Supplier<ExecutorService> s = Executors::newCachedThreadPool;").len(), 0);
    }

    #[test]
    fn new_thread_does_not_match_longer_identifiers() {
        assert_eq!(hits("new Thread(() -> work()).start();").len(), 1);
        assert_eq!(hits("new ThreadLocal<String>();").len(), 0);
        assert_eq!(hits("new ThreadPoolExecutor();").len(), 0);
        assert_eq!(hits("// new Thread(x)\nString s = \"new Thread(x)\";").len(), 0);
    }

    #[test]
    fn timer_matches_plain_and_qualified_names() {
        assert_eq!(hits("new Timer().schedule(task, 1000);").len(), 1);
        assert_eq!(hits("new java.util.Timer(true);").len(), 1);
        assert_eq!(hits("new TimerTask();").len(), 0);
    }

    #[test]
    fn ranges_point_at_the_offending_identifier() {
        let src = "Executors.newCachedThreadPool();";
        let d = &hits(src)[0];
        assert_eq!(d.range.start.character, 10);
        assert!(d.message.contains("newCachedThreadPool"));
    }

    fn date_formats(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        static_date_format(&Ctx::new(src))
    }

    #[test]
    fn static_date_format_fields_are_errors() {
        let got = date_formats("class A { static SimpleDateFormat f = new SimpleDateFormat(); }");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].severity, Severity::Error);
        assert_eq!(date_formats("class A { private static final DateFormat F = null; }").len(), 1);
        assert_eq!(date_formats("class A { static java.text.SimpleDateFormat f = null; }").len(), 1);
    }

    #[test]
    fn instance_fields_and_safe_types_are_left_alone() {
        assert_eq!(date_formats("class A { SimpleDateFormat f = new SimpleDateFormat(); }").len(), 0);
        assert_eq!(date_formats("class A { static DateTimeFormatter F = null; }").len(), 0);
        assert_eq!(date_formats("class A { static String f = \"yyyy\"; }").len(), 0);
    }

    #[test]
    fn a_local_variable_is_not_a_static_field() {
        assert_eq!(date_formats("class A { void f() { SimpleDateFormat f = new SimpleDateFormat(); } }").len(), 0);
        assert_eq!(date_formats("class A { static String s = \"SimpleDateFormat f\"; }").len(), 0);
    }

    fn randoms(src: &str) -> Vec<Diagnostic> {
        let _mock = MockHost::install();
        static_random(&Ctx::new(src))
    }

    #[test]
    fn static_random_fields_are_flagged() {
        assert_eq!(randoms("class A { static Random r = new Random(); }").len(), 1);
        assert_eq!(randoms("class A { private static final java.util.Random R = new Random(); }").len(), 1);
    }

    #[test]
    fn instance_and_thread_local_random_are_fine() {
        assert_eq!(randoms("class A { Random r = new Random(); }").len(), 0);
        assert_eq!(randoms("class A { static ThreadLocalRandom R = ThreadLocalRandom.current(); }").len(), 0);
        assert_eq!(randoms("class A { void f() { Random r = new Random(); } }").len(), 0);
        assert_eq!(randoms("class A { static String s = \"Random r\"; }").len(), 0);
    }
}
