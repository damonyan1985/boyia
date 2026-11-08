# Boyia 动态库 Builtin 插件

CLI 内置类默认静态链接在 `boyia_cli` 中。可选地通过 **cdylib 插件** 在运行时注册额外的全局类（例如 `Zip`）。插件与 Host 使用 **纯 Rust 函数指针** 注册，无需 C 结构体 ABI；`run_loop` / `thread_pool` 仍保留在 `boyia_cli`，异步调度通过 `BuiltinAsyncHost` trait 桥接。

## 约束

- Host（`boyia_cli`）与每个插件 **必须在同一 workspace、同一 Rust 工具链下一起编译**。
- 插件 dll **不能与 exe 版本混用**（无跨编译器二进制兼容保证）。
- 加载后 **不会 unload** 动态库（保证 `BuiltinRegistrar` 函数指针有效）。

## 工程结构

```
examples/boyia_plugin_sdk/      # BuiltinRegistrar、async/sync、SdkEmbedder
examples/boyia_plugin_sdk/macro/  # crate 名仍为 boyia_builtin_macro
examples/boyia_plugins/         # cdylib 插件集合
examples/boyia_plugins/boyia_zip_plugin/   # 示例 cdylib（Zip）
examples/boyia_plugins/boyia_path_plugin/  # 示例 cdylib（Path）
examples/boyia_cli/plugins/     # 可选：手动放置 dll（默认改由 SDK 布局，见下）
```

### `boyia_plugin_sdk` features

| Feature | 用途 |
|---------|------|
| `host-exe-plugins`（**default**） | Host 默认从 `{boyia_cli.exe}/plugins` 加载；关闭后回退为 cwd 下 `plugins/` |
| `build-install` | 插件 `[build-dependencies]` + 指向 SDK 共享 build 脚本（见下），将 cdylib 安装到 `target/{profile}/plugins/` |

插件 `Cargo.toml`（**无需** 自建 `build.rs`）：

```toml
build = "../boyia_plugin_sdk/build/plugin_install.rs"

[build-dependencies]
boyia_plugin_sdk = { path = "../boyia_plugin_sdk", features = ["build-install"] }
```

若 build 脚本还需其它逻辑，可自建 `build.rs` 并调用 `boyia_plugin_sdk::boyia_plugin_install_build!();`。

## 导出符号（插件侧）

由 `boyia_plugin_sdk::boyia_plugin_export!` 生成：

| 符号 | 含义 |
|------|------|
| `boyia_plugin_registrars_ptr` | 指向 `[BuiltinRegistrar]` 静态表 |
| `boyia_plugin_registrars_len` | 表长度 |
| `boyia_plugin_name` | 插件名（调试日志） |

## 环境变量

| 变量 | 默认 | 说明 |
|------|------|------|
| `BOYIA_ENABLE_DYLIB_BUILTINS` | `0` | 设为 `1` 时从插件目录加载 dll |
| `BOYIA_PLUGIN_DIRS` | `{exe}/plugins`（feature `host-exe-plugins`） | 多个目录用 `;`（Windows）或 `:`（Unix）分隔 |
| `BOYIA_PLUGIN_STRICT` | `0` | `1` 时任一插件加载失败则 CLI 退出 |
| `BOYIA_INIT_MINIMAL` | - | `1` 时不注册静态 builtin，也不加载插件 |

## 编写插件

1. 新建 `cdylib` crate，依赖 **`boyia_plugin_sdk`**（含 `boyia_class` 等宏再导出）及业务 crate（如 `zip`）；不必再直接依赖 `boyia_vm` / `boyia_gc`，也无需单独写 `boyia_builtin_macro` 依赖。
2. 用与 CLI 相同的方式编写 `#[boyia_class]` / `#[boyia_async_builtin]` 等（见 [语言开发文档](./boyia_language_development.md) §6）。
3. 在 `lib.rs` 中注册：

```rust
mod my_module;

boyia_plugin_sdk::boyia_plugin_export! {
    name: "boyia_my_plugin",
    registrars: [my_module::builtin_my_class],
}
```

4. 异步方法依赖 Host 注入：`boyia_cli` 启动时 `set_host_ctx_getter`，并在加载每个 cdylib 后调用导出的 `boyia_plugin_bind_host_ctx`（插件内 `BuiltinCtx::from_vm` 才能跨 DLL 使用线程池）。

参考实现：`examples/boyia_plugins/boyia_zip_plugin/src/zip_plugin.rs`（与 `examples/boyia_cli/src/builtins/utility/zip.rs` 逻辑相同，供 cdylib 构建）。

## 构建与部署

在仓库根目录：

```bash
cargo build -p boyia_plugin_sdk
cargo build -p boyia_cli
cargo build -p boyia_zip_plugin --release   # 或 debug：与 CLI 同 profile 更方便
```

插件 cdylib 构建完成后会自动复制到与 `boyia_cli.exe` **同目录下的 `plugins/`**：

- Debug: `target/debug/plugins/boyia_zip_plugin.dll`（与 `target/debug/boyia_cli.exe`）
- Release: `target/release/plugins/...`

CLI 默认从 **`{当前 boyia_cli 可执行文件所在目录}/plugins`** 扫描 dylib（可用 `BOYIA_PLUGIN_DIRS` 覆盖，多路径用 `;` 或 `:` 分隔）。无需再手动拷到 `examples/boyia_cli/plugins/`。

## 测试 Zip 插件

```powershell
# 仓库根目录
cargo build -p boyia_zip_plugin
cargo build -p boyia_cli
$env:BOYIA_ENABLE_DYLIB_BUILTINS = "1"
cargo run -p boyia_cli -- test/zip_plugin.boyia
```

```bash
cargo build -p boyia_zip_plugin
cargo build -p boyia_cli
BOYIA_ENABLE_DYLIB_BUILTINS=1 cargo run -p boyia_cli -- test/zip_plugin.boyia
```

`cargo run` 使用 `target/debug/boyia_cli.exe`，插件从同目录下的 `plugins/` 加载。

若不启用 `BOYIA_ENABLE_DYLIB_BUILTINS`，仅静态全局类 `Zip`（`utility/zip.rs`）可用。

启用插件时，`boyia_zip_plugin` 注册全局类 **`ZipPlugin`**（与静态 `Zip` 可同时注册）。插件测试：`test/zip_plugin.boyia`；静态 Zip：`test/zip.boyia`。

### 测试脚本

- `test/zip.boyia` — 静态 `Zip.compress` / `Zip.extract`（无需 dylib）。
- `test/zip_plugin.boyia` — 全局类 **`ZipPlugin`**（需 `BOYIA_ENABLE_DYLIB_BUILTINS=1`，且 `{boyia_cli.exe}/plugins/` 内有 dll）。

需存在 `test/assets/zip_fixture/`（含 `hello.txt`）；输出写入 `test/out/`。`main.rs` 会通过 `CARGO_MANIFEST_DIR` 将 cwd 设为 `examples/boyia_cli`。

### 静态 builtin 回归

不启用插件时，其它类仍由 `DEFAULT_BUILTINS` 注册，例如：

```bash
cd examples/boyia_cli
cargo run -- test/hashmap.boyia
```

## Host 实现要点

- `examples/boyia_cli/src/runner/plugin_loader.rs`：扫描目录、`libloading` 收集 registrar。
- `examples/boyia_cli/src/runner/cli_async_host.rs`：`impl BuiltinAsyncHost for CliBuiltinAsyncHost`。
- `boyia_plugin_sdk::set_host_ctx_getter`：避免 cdylib 内对 `SdkEmbedder` 的跨模块 `TypeId` downcast 失败。

## 延伸阅读

- [Boyia 语言开发文档 §6 Builtins](./boyia_language_development.md)
- [Builtin Native 对象](./builtin_struct_fields_mapping.md)
