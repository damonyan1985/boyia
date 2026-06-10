# Boyia Language

中国人自己开发的 OOP 脚本语言引擎，基于纯 Rust 实现，支持类、继承、异步与 Rust 原生扩展。可作为嵌入式脚本引擎，在 CLI 中已集成 `File`、`Https`、`Zip`、`Json` 等内置能力。

Boyia is a Rust-only OOP scripting language engine with custom syntax, class-based OOP, async/await, and native extensions written in Rust.

## 环境要求

- [Rust](https://www.rust-lang.org/tools/install)（stable，含 `cargo`）
- 网络访问（首次构建会拉取依赖；运行 `Https` 示例需联网）

克隆仓库后，在项目根目录操作：

```bash
git clone <your-repo-url>
cd boyia
```

## 快速运行 boyia_cli

`boyia_cli` 是官方示例运行器：启动 VM、注册 CLI 内置类、编译并执行 `examples/boyia_cli/script/main.boyia`。

```bash
# 在项目根目录
cargo run -p boyia_cli
```

Windows PowerShell 同样适用。首次编译会稍慢，之后会快很多。

成功时终端会先打印初始化日志，再输出脚本里 `Util.log` / `BY_Log` 的内容，最后出现 `Done.`。

### 调试选项

跳过 CLI 扩展内置类（仅保留 runtime 自带的 `String` / `Map` / `Array` / `MicroTask`），用于排查初始化问题：

```bash
# Linux / macOS
BOYIA_INIT_MINIMAL=1 cargo run -p boyia_cli

# Windows PowerShell
$env:BOYIA_INIT_MINIMAL="1"; cargo run -p boyia_cli
```

## 新人上手：编写第一个 Demo

当前 CLI **固定执行** `examples/boyia_cli/script/main.boyia`。上手最快的方式是直接改这个文件（或先备份一份再改）。

### 1. 最小 Hello World

将 `examples/boyia_cli/script/main.boyia` 暂时替换为：

```boyia
require("./util/util.boyia");

class Hello {
    prop fun run() {
        Util.log("Hello, Boyia!");
    }
};

var app = new(Hello);
app.run();
```

然后运行：

```bash
cargo run -p boyia_cli
```

应看到类似 `< Hello, Boyia! >` 的日志。

### 2. 使用类、Map 与内置 Json

```boyia
require("./util/util.boyia");

class Demo {
    prop fun run() {
        var user = {
            name: "boyia",
            score: 100
        };
        Util.log("name = " + user.name);

        var jsonText = "{\"ok\":true}";
        var obj = Json.parse(jsonText);
        Util.log("parsed ok = " + obj.ok);
        Util.log("back to json: " + Json.toString(obj));
    }
};

new(Demo).run();
```

### 3. 异步示例（File / Https）

完整异步、`async/await` 写法见仓库自带的 `main.boyia`（含 `File.read`、`Json.asyncParse`、`Https.load` 等）。模式一般是：

1. 用 `Util.newMicrotask` 包装 callback；
2. 调用 `File.read(path, resolve)` 等异步内置方法；
3. 在 `prop async` 方法里 `await` 结果 Map（`status` / `data` / `message`）。

```boyia
// 回调形态（节选）
File.read("vm_test.json", fun(res) {
    if (res.get("status") == "ok") {
        Util.log(res.get("data"));
    }
});
```

`vm_test.json` 已放在 `examples/boyia_cli/` 目录，可与 `File.read` 联调。

### 4. 模块引用

脚本可通过 `require` 加载同目录下的其他 `.boyia` 文件：

```boyia
require("./util/util.boyia");
```

路径相对于**当前被编译脚本文件**所在目录解析。

## CLI 内置类一览

| 类 | 说明 | 典型方法 |
|----|------|----------|
| `File` | 异步文件 IO | `read`, `write`, `exists`, `create`, `delete` … |
| `Https` | 异步 HTTP | `load`, `request` |
| `Zip` | 异步压缩/解压 | `compress`, `extract` |
| `Json` | JSON 同步 + 异步 | `parse`, `toString`, `asyncParse`, `asyncToString` |

Runtime 启动时还会自动注册：`String`、`Map`、`Array`、`MicroTask`（见 `crates/boyia_builtins`）。

## 仓库结构

```
boyia/
├── crates/
│   ├── boyia_vm/          # 虚拟机、编译与执行
│   ├── boyia_runtime/   # Runtime 生命周期、native 表
│   ├── boyia_builtins/  # 核心内置类（String/Map/Array/MicroTask）
│   ├── boyia_lib/       # 通用 native（new、BY_Log、require 等）
│   └── ...
├── examples/
│   ├── boyia_cli/       # ★ 推荐入口：运行脚本 + CLI 内置类
│   │   ├── script/main.boyia
│   │   └── src/builtins/utility/   # File/Https/Zip/Json 的 Rust 实现
│   └── boyia_lsp/       # .boyia 语言的 LSP 服务（可选）
└── tools/docs/
    └── boyia_language_development.md   # 语法与扩展开发文档
```

## 用 Rust 扩展 Boyia

- **CLI 内置类**：在 `examples/boyia_cli/src/builtins/utility/` 用 `#[boyia_class]`、`#[boyia_async_builtin]`、`#[boyia_sync_builtin]` 声明，并加入 `builtins/mod.rs` 的 `DEFAULT_BUILTINS`。
- **通用 native 函数**：参考 `crates/boyia_lib`，在 runtime 中注册到 native 表。
- **JSON 转换辅助**：`examples/boyia_cli/src/runner/macro/builtin_json.rs`。

详细步骤、宏约束与异步返回约定见开发文档。

## 其他命令

```bash
# 只检查 boyia_cli 能否编译
cargo check -p boyia_cli

# 构建 release 版 CLI
cargo build -p boyia_cli --release

# 语言服务（编辑器集成，可选）
cargo run -p boyia_lsp
```

## 文档

- [Boyia 语言开发文档](tools/docs/boyia_language_development.md) — 语法、`class`/`prop`/`async`、Builtins 编写、`Json` 与 `async/await` 示例

## Features

1. 支持 Rust 原生扩展：Builtins 类与 Native 函数均可接入。
2. 支持面向对象：`class`、继承、`prop` 属性与方法。
3. 支持异步：`async/await` + 线程池异步 Builtins（File / Https / Zip / Json 等）。
