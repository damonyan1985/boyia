# Boyia IDE（VS Code 扩展）

Boyia 脚本语言的编辑器支持扩展，为 `.boyia` 文件提供语法高亮、代码片段、智能补全、悬停提示、跳转定义与 `require` 路径链接。与本仓库 Rust 版 VM / CLI 配套使用。

**版本：** 0.0.7  
**扩展 ID：** `BoyiaIDE.boyia-ide`（本地开发时以 `package.json` 为准）

## 功能一览

| 能力 | 说明 |
|------|------|
| 语法高亮 | TextMate 语法 `syntaxes/boyia.tmLanguage.json`，语言 ID 为 `boyia` |
| 代码片段 | `config/snippets.json` |
| 主题 / 配色 | `src/theme/boyia-token-defaults.js`，构建前通过 `sync-theme-defaults` 写入 `package.json` |
| 智能补全 | 关键字、全局内置类、当前文件符号、`require` 引入的类、`.` 成员补全 |
| 悬停文档 | 内置 API 说明来自 `config/assist.json` 的 `apiDocs` |
| 跳转定义 | 类 / 方法 / 变量；`require("./x.boyia")` 可跳到目标文件 |
| 文档链接 | `require` 字符串在编辑器中可点击 |
| 日志面板 | 扩展内 `CodeLogView` 接管 `console.log`，便于调试扩展本身 |

> **调试器：** `src/code-debug/` 内含基于 `@vscode/debugadapter` 的 Boyia 调试会话实现（默认端口 `8888`），当前**尚未**在 `extension.js` 中注册。断点调试需后续接入 `contributes.debuggers` 并与运行时 WebSocket 联调。

## 环境要求

- [Node.js](https://nodejs.org/)（建议 LTS）
- [VS Code](https://code.visualstudio.com/) ≥ 1.36（`engines.vscode`: `^1.36.0`）

## 本地开发与安装

### 1. 安装依赖并构建

在仓库根目录或本目录执行：

```bash
cd tools/plugin
npm install
npm run build
```

构建产物为 `dist/extension.js`（webpack 打包 `src/extension.js` 及子模块）。

### 2. 在 VS Code 中调试扩展

1. 用 VS Code 打开 `tools/plugin` 目录（或打开 monorepo 根目录并指定 launch）。
2. 按 **F5** 启动「Extension Development Host」。
3. 在新窗口中打开任意 `.boyia` 文件，验证高亮与补全。

### 3. 打包为 VSIX（可选）

```bash
npm install -g @vscode/vsce
cd tools/plugin
vsce package
```

在 VS Code 中选择「从 VSIX 安装扩展」。

## 目录结构

```
tools/plugin/
├── package.json              # 扩展清单、语言/语法/片段贡献点
├── webpack.config.js
├── config/
│   ├── assist.json           # 补全命名空间与 API 悬停文案
│   └── snippets.json
├── syntaxes/
│   └── boyia.tmLanguage.json
├── language/
│   └── language.json         # 注释、括号等语言配置
├── icon/                     # 语言图标（亮/暗）
├── src/
│   ├── extension.js          # 入口：CodeAssist + CodeLog
│   ├── code-assist/          # 补全、悬停、定义、require 链接
│   ├── code-log/             # 扩展日志视图
│   ├── code-debug/           # 调试适配器（待接入）
│   ├── code-global/          # 扩展上下文
│   ├── code-util/
│   └── theme/
├── scripts/
│   └── sync-package-json-theme.js
└── test/                     # 扩展单元测试（mocha）
```

## 配置与扩展补全

### `config/assist.json`

- **`namespaces`**：按前缀列出静态方法名（如 `File.read`、`Json.parse`），供 `.` 补全使用。
- **`apiDocs`**：悬停时显示的简短说明。

新增 CLI 内置类（例如 `Tensor`）时，请同步在此文件增加 `Tensor.` 命名空间与对应 `apiDocs`，并在 `src/code-assist/CodeAssist.js` 的 `BUILTIN_GLOBAL_CLASSES` 中加入类名。

### 关联其它扩展名

首次激活时，`CodeAssist.linkBoyiaFile()` 会将 `*.boui` → `xml`、 `*.boss` → `css` 写入用户 `files.associations`（与 BoyiaEngine UI 资源配套）。

## 常用 npm 脚本

| 命令 | 作用 |
|------|------|
| `npm run build` | 同步主题默认值并 webpack 生产构建 |
| `npm run sync-theme-defaults` | 将 token 配色写回 `package.json` |
| `npm test` | 运行扩展测试 |

## 相关文档

- [Boyia 语言开发文档](../docs/boyia_language_development.md) — 语法、Builtins、`async/await`、Tensor 等
- [仓库 README](../../README.md) — `boyia_cli` 运行方式与 crate 结构

## 历史参考（C++ BoyiaEngine）

Boyia 语言最初在 BoyiaEngine 中实现；本仓库为 **Rust 重实现**（VM / Runtime / CLI）。以下链接仍可作为语法与调试协议参考：

- BoyiaEngine：<https://github.com/damonyan1985/BoyiaEngine>
- 旧版 VM 核心（C++）：<https://github.com/damonyan1985/BoyiaEngine/blob/dev/BoyiaFramework/source/boyia/kernel/vm/core/BoyiaCore.cpp>
- VS Code 调试扩展指南：<https://code.visualstudio.com/api/extension-guides/debugger-extension>

## 可选：LSP

本仓库还提供 `examples/boyia_lsp`（Rust 语言服务）。扩展当前以内置 `CodeAssist` 为主；若需更完整的语义分析，可另行对接 LSP 客户端。
