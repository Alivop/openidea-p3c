//! Java Coding Guidelines for OpenIDEA: checks Java files against the Alibaba
//! Java Development Manual (P3C), entirely in WebAssembly — no PMD, no JDK, no
//! external process.
//!
//! - 安装/启用/冷启动后自动报告已打开项目的问题（`on-startup-finished` 立即唤醒），无需手动扫描；
//! - 打开/编辑/保存/聚焦时即时刷新（push 模型，同官方 checkstyle/license-header），机械可判定的给快速修复；
//! - *Scan Project* 在后台任务里扫全项目，结果进 Problems 视图。

use openidea_plugin::host;
use openidea_plugin::providers::{CodeAction, Document, Providers};
use openidea_plugin::{Context, Error, Event, Invocation, NotificationLevel, Plugin, Project, Range, register_plugin};

mod fixes;
mod lex;
mod rules;
mod settings;

/// 一次扫描最多读的文件数。
const MAX_SCAN: u32 = 5_000;
/// 超过这个大小的文件跳过，与宿主对诊断文档的上限一致。
const MAX_FILE: usize = 2 << 20;

#[derive(Default)]
pub struct P3c;

impl P3c {
    /// 读一个文件的当前文本并发布诊断；read_file 未保存时返回编辑器缓冲，否则读盘。
    fn report(project: &Project, path: &str) -> Result<(), Error> {
        if !path.ends_with(".java") {
            return Ok(());
        }
        let text = project.read_file(path)?;
        if text.len() > MAX_FILE {
            return Ok(());
        }
        host::publish_diagnostics(path, &rules::analyze(&text))
    }

    /// 返回 (检查过的文件数, 有问题的文件数)。
    fn scan(project: &Project, zh: bool) -> Result<(u32, u32), Error> {
        let files = project.find_files("**/*.java", MAX_SCAN)?;
        let total = files.len() as u32;
        let title = if zh { "Java Coding Guidelines：扫描项目" } else { "Java Coding Guidelines: scan project" };
        let task = host::begin_task(title, true);
        let (mut checked, mut found) = (0, 0);
        for (i, path) in files.iter().enumerate() {
            if task.is_cancelled() {
                break;
            }
            if i % 20 == 0 {
                task.report(i as u32, Some(total), Some(path.rsplit('/').next().unwrap_or(path)));
            }
            let Ok(text) = project.read_file(path) else { continue };
            if text.len() > MAX_FILE {
                continue;
            }
            checked += 1;
            let items = rules::analyze(&text);
            if !items.is_empty() {
                found += 1;
            }
            host::publish_diagnostics(path, &items)?;
        }
        Ok((checked, found))
    }

    /// 激活或定时唤醒时静默重发所有打开项目的 Java 诊断：安装/启用后无需手动扫描，
    /// 已打开的文件立即出现问题。按当前 UI 语言生成，遵循总开关；单一发布源，不与扫描重复。
    /// 全项目 sweep 可能远超普通调用的 5s/fuel 预算，必须挂在报告任务里：宿主按 report 续期。
    fn republish_open_projects() {
        let Ok(projects) = host::projects() else { return };
        let zh = host::locale().starts_with("zh");
        let title = if zh { "Java Coding Guidelines：刷新诊断" } else { "Java Coding Guidelines: refresh diagnostics" };
        let task = host::begin_task(title, true);
        let mut done = 0u32;
        for project in projects {
            let Ok(files) = project.find_files("**/*.java", MAX_SCAN) else { continue };
            for path in files {
                if task.is_cancelled() {
                    return;
                }
                // 每文件都 report：单个大文件的 analyze 也不会让两次续期之间超预算。
                task.report(done, None, Some(path.rsplit('/').next().unwrap_or(&path)));
                let Ok(text) = project.read_file(&path) else { continue };
                if text.len() > MAX_FILE {
                    continue;
                }
                let _ = host::publish_diagnostics(&path, &rules::analyze(&text));
                done += 1;
            }
        }
    }
}

impl Plugin for P3c {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        // 订阅 Java 文件的键入事件；需要 editor.read，未授予时忽略（仍按打开/保存/聚焦报告）。
        let _ = host::subscribe_paths("document-changed", &["**/*.java"]);
        // on-startup-finished 让安装/启用/冷启动立即唤醒插件：静默重发，无需手动扫描。
        Self::republish_open_projects();
        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "scan" => {
                let project = ctx.project().ok_or_else(|| Error::Invalid("open a project first".into()))?;
                let zh = host::locale().starts_with("zh");
                let (checked, found) = Self::scan(&project, zh)?;
                let body = if zh {
                    match found {
                        0 => format!("检查了 {checked} 个 Java 文件，未发现问题。"),
                        n => format!("检查了 {checked} 个 Java 文件，其中 {n} 个存在问题，详见 Problems 视图。"),
                    }
                } else {
                    match found {
                        0 => format!("Checked {checked} Java files, no problems found."),
                        n => format!(
                            "Checked {checked} Java files, {n} of them have problems; see the Problems view."
                        ),
                    }
                };
                host::notify(NotificationLevel::Info, "Java Coding Guidelines", &body);
                Ok(None)
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        for e in events {
            // 定时唤醒（空闲后 ≥60s）：按当前 UI 语言静默重发，作为切换语言的自动兜底。
            if matches!(e, Event::Scheduled) {
                Self::republish_open_projects();
                continue;
            }
            let path = match e {
                Event::DocumentChanged(c) => &c.path,
                Event::FileOpened(p) | Event::FileSaved(p) => p,
                // 聚焦到另一个文件时按当前语言重发。
                Event::EditorFocused(Some(p)) => p,
                _ => continue,
            };
            // document-changed 的路径是项目相对的，其余是绝对的；发布诊断需要绝对路径。
            let (project, abs) = match host::project_of(path) {
                Some(p) => (p, path.to_owned()),
                None => {
                    let Some(p) = ctx.project() else { continue };
                    let Ok(root) = p.root() else { continue };
                    (p, format!("{}/{}", root.trim_end_matches('/'), path.trim_start_matches('/')))
                }
            };
            let _ = Self::report(&project, &abs);
        }
    }
}

impl Providers for P3c {
    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        // 与 report 同上限：超大文件的 analyze 会吃满单次调用预算，快速修复不值得崩掉插件。
        if doc.text.len() > MAX_FILE {
            return Vec::new();
        }
        fixes::actions(&rules::Ctx::new(&doc.text), &doc.path, selection)
    }
}

register_plugin!(P3c, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{DocumentChanged, Severity};

    fn changed(path: &str) -> Event {
        Event::DocumentChanged(DocumentChanged { path: path.into(), ..Default::default() })
    }

    #[test]
    fn opening_a_file_reports_it() {
        let mock = MockHost::install().with_capabilities(&["project.read", "events"]);
        mock.add_file("src/A.java", "class A { long n = 2l; }");
        P3c.handle_events(&[Event::FileOpened("/project/src/A.java".into())], &mock.invocation());
        assert_eq!(mock.diagnostics()["/project/src/A.java"].len(), 1);
    }

    #[test]
    fn a_clean_file_yields_no_problems() {
        let mock = MockHost::install().with_capabilities(&["project.read", "events"]);
        mock.add_file("src/A.java", "class A {}");
        P3c.handle_events(&[Event::FileOpened("/project/src/A.java".into())], &mock.invocation());
        assert!(mock.diagnostics().get("/project/src/A.java").is_none_or(Vec::is_empty));
    }

    #[test]
    fn an_edited_literal_reaches_the_problems_view() {
        let mock = MockHost::install().with_capabilities(&["project.read", "events", "editor.read"]);
        mock.open_editor("/project/A.java", "java", "class A { long n = 2", &[]);
        // Typing the lowercase suffix is enough: no save, no host process.
        mock.type_in_editor("l; }");
        P3c.handle_events(&[changed("A.java")], &mock.invocation());
        let found = &mock.diagnostics()["/project/A.java"];
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("p3c.constant.long-literal-suffix"));
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].range.start.line, 0);
    }

    #[test]
    fn typing_a_comment_is_not_flagged() {
        let mock = MockHost::install().with_capabilities(&["project.read", "events", "editor.read"]);
        mock.open_editor("/project/A.java", "java", "class A { long n = 2L; }", &[]);
        mock.type_in_editor(" // 2l");
        P3c.handle_events(&[changed("A.java")], &mock.invocation());
        assert!(mock.diagnostics().get("/project/A.java").is_none_or(Vec::is_empty), "a comment must not be flagged");
    }

    #[test]
    fn focusing_republishes_in_the_ui_language() {
        let mock = MockHost::install().with_capabilities(&["project.read", "events"]);
        mock.add_file("src/A.java", "class A { long n = 2l; }");
        mock.set_locale("zh-CN");
        P3c.handle_events(&[Event::EditorFocused(Some("/project/src/A.java".into()))], &mock.invocation());
        assert!(mock.diagnostics()["/project/src/A.java"][0].message.contains("大写 L"));

        // Switching the UI language and focusing again must re-report in English.
        mock.set_locale("en");
        P3c.handle_events(&[Event::EditorFocused(Some("/project/src/A.java".into()))], &mock.invocation());
        assert!(mock.diagnostics()["/project/src/A.java"][0].message.contains("uppercase L"));
    }

    #[test]
    fn the_scan_reports_every_java_file_in_a_task() {
        let mock = MockHost::install().with_capabilities(&["project.read"]);
        mock.add_file("src/A.java", "class A { long n = 2l; }");
        mock.add_file("src/B.java", "class B { long n = 2L; }");
        mock.add_file("notes.md", "long n = 2l;");
        assert_eq!(P3c.run_command("scan", &mock.invocation(), None).unwrap(), None);

        let d = mock.diagnostics();
        assert_eq!(d["/project/src/A.java"].len(), 1);
        assert!(d.get("/project/src/B.java").is_none_or(Vec::is_empty));
        assert!(!d.contains_key("/project/notes.md"));
        assert_eq!(mock.tasks().len(), 1);
        assert!(
            mock.notifications()[0].body.contains("Checked 2 Java files, 1 of them"),
            "{:?}",
            mock.notifications()
        );
    }

    #[test]
    fn the_scan_reports_an_open_file_once() {
        let mock = MockHost::install().with_capabilities(&["project.read", "editor.read"]);
        mock.open_editor("/project/src/A.java", "java", "class A { long n = 2l; }", &[]);
        mock.add_file("src/A.java", "class A { long n = 2l; }");
        mock.add_file("src/B.java", "class B { long n = 2l; }");
        P3c.run_command("scan", &mock.invocation(), None).unwrap();
        // One producer only: the open file is reported once, never duplicated.
        assert_eq!(mock.diagnostics()["/project/src/A.java"].len(), 1);
        assert_eq!(mock.diagnostics()["/project/src/B.java"].len(), 1);
    }

    #[test]
    fn the_scan_speaks_the_ui_language() {
        let mock = MockHost::install().with_capabilities(&["project.read"]);
        mock.set_locale("zh-CN");
        mock.add_file("src/A.java", "class A { long n = 2l; }");
        P3c.run_command("scan", &mock.invocation(), None).unwrap();
        assert!(mock.notifications()[0].body.contains("1 个 Java 文件，其中 1 个"), "{:?}", mock.notifications());
        assert_eq!(mock.tasks()[0].title, "Java Coding Guidelines：扫描项目");
    }

    #[test]
    fn the_scan_respects_the_master_switch() {
        let mock = MockHost::install().with_capabilities(&["project.read"]);
        mock.set_setting("enabled", "false");
        mock.add_file("src/A.java", "class A { long n = 2l; }");
        P3c.run_command("scan", &mock.invocation(), None).unwrap();
        assert!(mock.diagnostics().get("/project/src/A.java").is_none_or(Vec::is_empty));
        assert!(mock.notifications()[0].body.contains("no problems found"), "{:?}", mock.notifications());
    }

    #[test]
    fn an_unknown_command_is_not_found() {
        let mock = MockHost::install();
        assert!(matches!(P3c.run_command("nope", &mock.invocation(), None), Err(Error::NotFound(_))));
    }

    #[test]
    fn activating_republishes_open_projects_without_a_scan() {
        let mock = MockHost::install().with_capabilities(&["project.read", "events"]);
        mock.add_file("src/A.java", "class A { long n = 2l; }");
        mock.add_file("src/B.java", "class B { long n = 2L; }");
        // No scan command: activation alone reports the open project's files.
        P3c.activate(&mock.context("alivop.p3c")).unwrap();
        assert_eq!(mock.diagnostics()["/project/src/A.java"].len(), 1);
        assert!(mock.diagnostics().get("/project/src/B.java").is_none_or(Vec::is_empty));
        assert!(mock.notifications().is_empty(), "activation must not run the scan command");
    }

    #[test]
    fn activation_republishes_inside_a_reporting_task() {
        let mock = MockHost::install().with_capabilities(&["project.read", "events"]);
        for i in 0..60 {
            mock.add_file(&format!("src/m{i}/S{i}.java"), "class S { long n = 2l; }");
        }
        P3c.activate(&mock.context("alivop.p3c")).unwrap();
        let tasks = mock.tasks();
        assert_eq!(tasks.len(), 1, "the whole-project republish must run in a task: only a reporting task gets the extended budget");
        assert!(tasks[0].ended, "the republish task must end with the call");
        assert!(
            tasks[0].reports.len() >= 3,
            "periodic reports renew the background budget: {:?}",
            tasks[0].reports
        );
        assert_eq!(mock.diagnostics().len(), 60);
        assert!(mock.notifications().is_empty(), "republish stays silent");
    }

    #[test]
    fn code_actions_skip_oversized_documents() {
        let _mock = MockHost::install().with_capabilities(&["project.read"]);
        let mut text = String::from("class Big { long n = 2l; }\n");
        while text.len() <= MAX_FILE {
            text.push_str("// padding padding padding padding padding\n");
        }
        let doc = Document { path: "/project/Big.java".into(), language: "java".into(), version: 1, text };
        let selection = Range {
            start: openidea_plugin::Position { line: 0, character: 0 },
            end: openidea_plugin::Position { line: 0, character: 40 },
        };
        assert!(P3c.code_actions(&doc, selection).is_empty(), "oversized documents must not run analyze in a provider call");
    }

    #[test]
    fn a_scheduled_wakeup_republishes_in_the_ui_language() {
        let mock = MockHost::install().with_capabilities(&["project.read", "events"]);
        mock.add_file("src/A.java", "class A { long n = 2l; }");
        mock.set_locale("zh-CN");
        P3c.handle_events(&[Event::Scheduled], &mock.invocation());
        assert!(mock.diagnostics()["/project/src/A.java"][0].message.contains("大写 L"));

        // The idle wakeup is the automatic fallback after a UI language switch.
        mock.set_locale("en");
        P3c.handle_events(&[Event::Scheduled], &mock.invocation());
        assert!(mock.diagnostics()["/project/src/A.java"][0].message.contains("uppercase L"));
    }

    #[test]
    fn activating_respects_the_master_switch() {
        let mock = MockHost::install().with_capabilities(&["project.read", "events"]);
        mock.set_setting("enabled", "false");
        mock.add_file("src/A.java", "class A { long n = 2l; }");
        P3c.activate(&mock.context("alivop.p3c")).unwrap();
        assert!(mock.diagnostics().get("/project/src/A.java").is_none_or(Vec::is_empty));
    }
}
