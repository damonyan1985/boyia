# Boyia Persistent Callback 宏改造方案（onReceive）

本文整理 `WebSocketServerBuiltins.onReceive` 持续回调能力的宏化改造方案，目标是：

- `onReceive` 脚本侧保持 `onReceive(fun(port, msg) { ... })`。
- `onReceive` 本身非阻塞，仅做监听注册。
- 尽量减少 builtin 业务代码里显式 `AsyncCtx` / `ScriptCallback` / `dispatch_on_receive` 模板。
- 高性能方向：减少跨线程 hop，避免 `recv_timeout` 轮询线程。
- 不改 `crates/*`，仅改 `examples/boyia_cli` 宏与 runner 层。

---

## 1. 现状与痛点

当前已支持：

- `#[boyia_sync_builtin(method = "...", callback = "persistent")]`
- sync handler 自动捕获 callback（脚本最后一个参数）。

但业务层仍有重复样板：

- `WebSocketServerBuiltins` 手写 `on_receive_ctx`、`on_receive_cb` 字段。
- 手写 `release_on_receive_callback`。
- 手写 `dispatch_on_receive`（回 runtime 线程 + 组参数 + 调 persistent callback）。
- 事件链路存在额外 hop（IO -> inbox -> onReceive 线程 -> runtime），高频消息下会增加调度开销。

目标是把这些模板下沉到宏/基础设施。

---

## 2. 关键约束

1. 仅靠 `#[boyia_sync_builtin(...)]`（方法级属性）**无法直接安全修改 struct 字段**。  
   `#[boyia_class]` 处理的是 `impl` AST，不直接控制 `#[boyia_native_object]` 的字段定义。

2. 要自动注入隐藏字段，需要 struct 级声明给宏一个“映射源”。

---

## 3. 推荐方案：双宏协同

### 3.1 struct 侧声明持久回调槽位（映射）

在 `#[boyia_native_object]` 上增加可选参数：

```rust
#[boyia_native_object(persistent_callbacks = ["onReceive"])]
pub struct WebSocketServerBuiltins { ... }
```

语义：

- 为每个名称自动生成隐藏字段（例如 `__boyia_cb_onReceive_ctx` / `__boyia_cb_onReceive`）。
- 自动初始化为 `None`。
- 自动生成内部 helper（`bind/release/invoke` 入口或访问器）。

### 3.2 method 侧声明行为

保留：

```rust
#[boyia_sync_builtin(method = "onReceive", callback = "persistent")]
```

语义增强：

- 宏展开时检查：`method = "onReceive"` 必须在 `persistent_callbacks` 映射中声明。
- 自动 capture callback 并写入对应隐藏槽位。
- 生成 `dispatch_onReceive` 辅助函数（或内联代码）用于持续触发，不要求业务手写。

---

## 3.3 性能优先事件链路（推荐）

推荐把 `onReceive` 事件路径改成：

`tokio websocket task -> runtime task -> script callback`

而不是：

`tokio websocket task -> std::sync::mpsc inbox -> recv_timeout 线程 -> runtime task -> script callback`

收益：

- 少一次线程切换与队列搬运；
- 去掉 `recv_timeout` 轮询；
- stop/offReceive 时只需关闭事件发射标记并释放 callback，不依赖超时轮询退出。

实现建议：

1. `receive()`（阻塞接口）保留现有 inbox 通道，兼容旧脚本。
2. `onReceive` 事件触发改为在 `handle_connection` 收到消息后直接调用宏生成的 `emit`。
3. `emit` 内部只做一件事：`ctx.post_runtime_task(...)` 并 `invoke_script_callback_persistent`。

---

## 4. onReceive 的推荐形态

### 4.1 对外 API（脚本不变）

```boyia
server.onReceive(fun(port, msg) {
    server.send(port, "echo: " + msg);
});
```

### 4.2 Rust builtin 业务代码（目标形态）

业务函数中不再显式暴露 `AsyncCtx` / `ScriptCallback` 参数，也不手写 dispatch 模板；
业务侧只描述监听开启/关闭与消息来源。

> 备注：若短期内无法一步到位，可先保留当前签名，再逐步迁移到纯业务签名。

---

## 5. 自动生成内容清单

当同时满足：

- `#[boyia_native_object(persistent_callbacks = ["onReceive"])]`
- `#[boyia_sync_builtin(method = "onReceive", callback = "persistent")]`

宏应自动生成：

1. **隐藏字段**：ctx/callback 存储。
2. **注册逻辑**：capture callback、覆盖旧值、必要时释放旧捕获。
3. **触发逻辑**：回 runtime 线程，tuple 参数转 `BoyiaValue`，调用 `invoke_script_callback_persistent`。
4. **释放逻辑**：`stop/drop/offReceive` 可复用的 release helper。
5. **编译期校验**：
   - method 未声明映射时报错；
   - `persistent` 与 tuple one-shot 回调模式冲突时报错；
   - 参数/返回签名不匹配时报错。

---

## 6. 迁移步骤（建议）

1. **第一步（已具备基础）**  
   保持现有 `callback = "persistent"`，稳定功能。

2. **第二步**  
   给 `boyia_native_object` 增加 `persistent_callbacks` 配置与隐藏字段生成。

3. **第三步**  
   在 `boyia_class` 展开中接入 mapping 校验与自动 dispatch/release 包装。

4. **第四步**  
   清理 `ws_server.rs` 手写的 `on_receive_ctx`、`on_receive_cb`、`dispatch_on_receive`、`release_on_receive_callback`。

---

## 6.1 可实施流程（按文件拆解）

下面流程是可以按提交粒度直接执行的最小实现路径。

### A. `runner/builtin/builtin_macro.rs`

1. 给 `boyia_native_object` 增加可选参数解析：
   - `persistent_callbacks = ["onReceive", "onClose"]`
2. 为每个回调名生成隐藏字段：
   - `Option<AsyncCtx>`
   - `Option<ScriptCallback>`
3. 生成隐藏 helper（建议 `impl` 私有函数）：
   - `__boyia_bind_<event>(ctx, cb)`
   - `__boyia_release_<event>()`
   - `__boyia_emit_<event>(arg1, arg2, ...)`（可选，或由 `boyia_class` 生成）
4. 在 `expand_sync_method` 中处理：
   - `callback = "persistent"` 时，要求对应 `method` 在 `persistent_callbacks` 中声明；
   - 自动 `capture_callback()`，避免业务函数暴露 `ScriptCallback` 参数；
   - 自动注入 `AsyncCtx`（由 `async_ctx_from_vm` 获取）；
   - 拒绝与 tuple one-shot 回调混用。

### B. `runner/builtin/builtin_async.rs`

保留并复用现有能力：

- `invoke_script_callback_persistent`
- `release_script_callback`
- `AsyncCtx::post_runtime_task`

可选增强：

- 新增一个通用 `dispatch_persistent_tuple(ctx, cb, args)` 函数，减少宏展开重复代码。

### C. `builtins/external/ws_server.rs`

1. struct 上改为声明式配置（不再手写 `on_receive_ctx/on_receive_cb`）；
2. `on_receive` 函数只保留业务逻辑（注册/开关事件发射状态）；
3. 在 websocket 消息入口（`handle_connection` 收到 Text/Binary）直接调用宏生成 helper（如 `self.__boyia_emit_onReceive(port, msg)`）；
4. 不再新增 `recv_timeout` 监听线程；
5. `offReceive/stop/drop` 调用宏生成 release helper（如 `self.__boyia_release_onReceive()`）。

---

## 6.2 代码示例（迁移前后）

### 示例 1：native object 声明（目标）

```rust
#[boyia_native_object(persistent_callbacks = ["onReceive"])]
pub struct WebSocketServerBuiltins {
    #[boyia_field_default = "0.0.0.0"]
    host: String,
    #[boyia_field_default = "0"]
    port: u64,
    #[boyia_field_default = "false"]
    running: bool,
    #[boyia_field(skip)]
    runtime: Option<ServerRuntime>,
    // onReceive 持久回调隐藏字段由宏自动生成
}
```

### 示例 2：sync builtin 声明（目标）

```rust
#[boyia_class(name = "WebSocketServer", registrar = builtin_websocket_server_class)]
impl WebSocketServerBuiltins {
    #[boyia_sync_builtin(method = "onReceive", callback = "persistent")]
    fn on_receive(&mut self) -> (u16, String) {
        // 元组仅作为 callback 参数签名声明：fun(port, msg) { ... }
        // 不作为同步返回值回传给脚本（persistent 模式由宏包装）。
        // 注册成功与否由宏侧 callback 绑定与业务侧运行态共同决定。
        (0, String::new())
    }
}
```

> 说明：`callback = "persistent"` 下，`(u16, String)` 语义是事件参数 schema，
> 不是脚本侧同步返回值。

### 示例 3：宏展开后的等价伪代码（核心）

```rust
fn on_receive_handler(site: &mut SyncCallSite<'_>) -> OpHandleResult {
    let class_body = some_or_end!(site.this_function());
    let rt = unsafe { boyia_vm::get_runtime_from_vm(site.vm()) };
    if rt.is_null() {
        return OpHandleResult::kOpResultEnd;
    }
    let state = unsafe { boyia_gc::boyia_native_mut::<WebSocketServerBuiltins>(class_body, &mut *rt) };

    // 自动捕获 callback + ctx（persistent 模式）
    let __cb = some_or_end!(site.capture_callback());
    let __ctx = some_or_end!(crate::runner::builtin_async::async_ctx_from_vm(site.vm()));
    state.__boyia_bind_onReceive(__ctx, __cb);

    let _schema = WebSocketServerBuiltins::on_receive(state);
    crate::runner::builtin_sync::set_sync_return((), site.vm())
}
```

### 示例 4：事件触发伪代码（宏生成 helper）

```rust
impl WebSocketServerBuiltins {
    fn __boyia_emit_onReceive(&self, port: u16, msg: String) {
        let (Some(ctx), Some(cb)) = (
            self.__boyia_cb_onReceive_ctx.clone(),
            self.__boyia_cb_onReceive.clone(),
        ) else {
            return;
        };
        let _ = ctx.post_runtime_task(move |runtime| {
            let vm_ptr = runtime.vm();
            if vm_ptr.is_null() { return; }
            let Some(vm) = (unsafe { boyia_vm::vm_from_void(vm_ptr) }) else { return; };
            let Some(a0) = crate::runner::builtin_sync::push_callback_int(port as i64, vm) else { return; };
            let Some(a1) = crate::runner::builtin_sync::push_callback_string(msg, vm) else { return; };
            let mut args = vec![a0, a1];
            let _ = crate::runner::builtin_async::invoke_script_callback_persistent(vm, cb, &mut args);
        });
    }
}
```

### 示例 5：WebSocket 消息入口（高性能路径）

```rust
match incoming {
    Some(Ok(Message::Text(text))) => {
        enqueue_text(&inbox_tx, client_port, text.to_string()); // 兼容 receive()
        if let Some(server_ref) = server_state.upgrade() {
            server_ref.__boyia_emit_onReceive(client_port, text.to_string());
        }
    }
    Some(Ok(Message::Binary(bytes))) => {
        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
            enqueue_text(&inbox_tx, client_port, text.clone());
            if let Some(server_ref) = server_state.upgrade() {
                server_ref.__boyia_emit_onReceive(client_port, text);
            }
        }
    }
    _ => {}
}
```

---

## 6.3 编译期校验规则（必须实现）

1. `callback = "persistent"` 必须有 `method = "..."`。
2. `method` 必须出现在 `persistent_callbacks` 列表里。
3. `callback = "persistent"` 不能与“tuple one-shot 回调返回”同时启用。
4. `persistent_callbacks` 里出现重复名称时报错。
5. 映射名建议严格区分大小写（`onReceive` != `onreceive`）。

建议报错格式：

```text
`#[boyia_sync_builtin(method = "onReceive", callback = "persistent")]` requires
`onReceive` to be declared in `#[boyia_native_object(persistent_callbacks = [...])]`
```

---

## 6.4 测试与验收脚本建议

### smoke 脚本（已有）

- `examples/boyia_cli/test/ws_on_receive_smoke.boyia`

### 增补建议

1. **重复注册覆盖**
   - 连续调用两次 `onReceive`，确保只触发最后一次 callback。
2. **offReceive 生效**
   - 调用 `offReceive` 后不再触发 callback。
3. **stop/drop 释放**
   - `stop` 后回调不再触发；对象析构后无悬挂调用。
4. **错误路径**
   - 未 `start` 就 `onReceive` 返回 false；
   - 映射缺失时编译期报错（宏单测）。

命令：

```bash
cargo build --manifest-path examples/boyia_cli/Cargo.toml
cargo run --quiet --manifest-path examples/boyia_cli/Cargo.toml -- examples/boyia_cli/test/ws_on_receive_smoke.boyia
```

---

## 7. 风险与注意点

- 生命周期：覆盖注册/stop/drop/offReceive 必须严格释放 persistent callback，避免捕获泄漏。
- 线程模型：callback 必须在 runtime 线程触发，不可在 IO 线程直接调用 VM。
- 并发与所有权：从 tokio 任务直接发射事件时，需保证可安全访问 native object（建议只传递可克隆事件句柄，不跨线程持有 `&mut self`）。
- 背压：高频消息下建议引入有界队列或批量投递策略，避免 runtime 任务队列无限膨胀。
- 命名稳定性：method 名与 mapping key（`onReceive`）需统一，建议严格大小写匹配并编译时报错。
- 向后兼容：保留当前非 persistent sync/async 宏语义，不影响既有 builtin。

---

## 8. 验收标准

- `onReceive` 在脚本中可持续收到回调，且方法调用本身不阻塞。
- `offReceive` / `stop` / `drop` 后无回调继续触发。
- builtin 业务文件中不再出现重复 callback 存储与 dispatch 模板代码。
- `cargo build --manifest-path examples/boyia_cli/Cargo.toml` 通过。

