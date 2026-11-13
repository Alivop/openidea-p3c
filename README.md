# Java Coding Guidelines (P3C) — OpenIDEA plugin

Checks Java files against the **Alibaba Java Development Manual (P3C)** while you
type, with quick fixes and a project-wide scan. Diagnostics appear automatically
once the plugin is installed or enabled, and refresh as you open, edit and save.

It is **pure WebAssembly**: a self-contained tokenizer and shallow declaration
parser written in Rust. There is **no PMD, no JDK and no external process** — nothing
is downloaded or executed at analysis time, so it stays fast and works offline.

- **Plugin id:** `alivop.p3c`
- **Requires:** OpenIDEA with plugin API `^1.6` (OpenIDEA 0.1.43 or newer)
- **License:** Apache-2.0

## What it checks

38 handbook rules in 9 groups, with 6 quick fixes. Each group can be switched on or
off, and there is a master switch.

| Group | Rules | Examples |
| --- | --- | --- |
| Naming Style | 11 | UpperCamelCase types, lowerCamelCase members, `UPPER_SNAKE` constants, array brackets next to the type, no `is` prefix on boolean fields, lowercase packages, `Test` suffix, method length limit |
| Other | 6 | `yyyy` not `YYYY` in date patterns, `System.currentTimeMillis`, no Apache `BeanUtils`, no `Pattern.compile` in a method body, no `(int) Math.random()`, no `==` on floats |
| Concurrency | 5 | no `Executors` thread pools, no explicit `new Thread`, no `Timer`, no static `SimpleDateFormat`, no static `Random` |
| Flow Control | 5 | braces on `if/for/while/do`, `default` in `switch`, no `return` in `finally`, no add/remove inside foreach, no `+` string concatenation in a loop |
| Collections | 4 | correctly typed `toArray`, no mutation of `Arrays.asList`, no cast of `subList` to `ArrayList`, initial capacity for hash collections (opt-in) |
| Comments | 3 | Javadoc on abstract/interface methods, comments on enum constants, `@author` on classes (all opt-in) |
| OOP | 2 | string constant / known-non-null on the left of `equals`, no `BigDecimal(double)` |
| Constants | 1 | uppercase `L` suffix on long literals |
| Exceptions | 1 | `@Transactional` should specify `rollbackFor` |

The four **Comments** rules and **Collections → initial capacity** are **off by
default** so they don't flood a legacy project; enable them per group when you want
them. The method-length threshold defaults to 80 lines (configurable).

## Using it

- **Settings:** `Settings → Plugins → Java Coding Guidelines` — master switch,
  per-group switches, the opt-in rules, and the method-length threshold.
- **Scan the whole project:** `Tools → Java Coding Guidelines: Scan Project`
  (also in Find Action).
- **Quick fixes:** available on the diagnostics that support them.

The plugin name, settings and diagnostics follow the IDE's display language
(English / 简体中文).

## Permissions

The plugin asks only for low-risk capabilities, and no high-risk ones:

| Capability | Required? | Why |
| --- | --- | --- |
| `project.read` | required | read the project's Java files to scan them |
| `events` | required | refresh diagnostics when you open / edit / save / focus a file |
| `editor.read` | optional | read the focused editor's file and selection for live diagnostics and quick fixes |

There is **no `network:`, `process:`, `fs.write:`, `ipc:` or `ide.commands`**
capability: the plugin never reaches the network, spawns a process, or writes files.

## Install

Once listed in the marketplace: `Settings → Plugins → Available`, search for
**Java Coding Guidelines**, install, and allow the permissions.

From a package file: `Settings → Plugins →` gear menu `→ Install Plugin from
Package File…`, then choose `alivop.p3c-<version>.oiplugin`.

## Build from source

Prerequisites: Rust through [rustup](https://rustup.rs) and the `openidea-plugin`
tool (from an OpenIDEA release, or
`cargo install --locked --git https://github.com/l0ng-ai/openidea ide-host --bin openidea-plugin`).
The SDK is pinned to the `v0.1.43` tag in `Cargo.toml`.

```sh
cargo test                 # 165 unit tests against the SDK's mock host
openidea-plugin build      # cargo build --release --target wasm32-wasip2 -> dist/plugin.wasm, then check
openidea-plugin pack . --key alivop.key --publisher alivop   # signed alivop.p3c-0.1.0.oiplugin
```

`alivop.key` is the publisher's private Ed25519 key (`openidea-plugin keygen
alivop.key`). Keep it secret; it is gitignored and must never be committed.

## 中文说明

边写边按《阿里巴巴 Java 开发手册》（P3C）检查 Java 文件，提供快速修复与全项目扫描；
安装或启用后自动出诊断，打开 / 编辑 / 保存时即时刷新。**纯 WebAssembly**（Rust 实现的
词法分析 + 轻量声明解析），**无需 PMD / JDK / 外部进程**，分析时不下载、不执行任何外部
程序，快速且可离线使用。

- **38 条手册规则（9 组）、6 个快速修复**：命名 11、其他 6、并发 5、流程控制 5、集合 4、
  注释 3、OOP 2、常量 1、异常 1。
- **注释类 3 条**与**集合初始容量**默认关闭，避免在遗留项目里刷屏；按需开启。方法行数
  阈值默认 80（可配置）。
- **设置路径：** 设置 → 插件 → Java Coding Guidelines（总开关 + 按组开关 + 阈值）。
- **扫描路径：** 工具 → Java Coding Guidelines: Scan Project（也可在 Find Action 中运行）。
- **权限：** 仅低风险 `project.read`、`events`（必需）与 `editor.read`（可选）；
  **不含任何 network / process / fs.write / ipc / ide.commands 高风险权限**。
- 插件名称、设置与诊断跟随 IDE 显示语言（English / 简体中文）。

从源码构建：`cargo test` → `openidea-plugin build` →
`openidea-plugin pack . --key alivop.key --publisher alivop`。私钥 `alivop.key` 须保密，
已被 gitignore，切勿提交。

## License

Apache-2.0. See [LICENSE](LICENSE).