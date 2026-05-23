# Boyia 语言开发文档

本文面向 Boyia 脚本开发者和 Rust 扩展开发者，说明常用语法与扩展机制。

## 1. 变量声明

Boyia 使用 `var` 声明变量：

```boyia
var name = "boyia";
var a = 100, b = 200;
var arr = ["1", "2"];
var obj = {
    key: "value",
    count: 3
};
```

说明：

- 支持一行声明多个变量（逗号分隔）。
- 支持字符串、数字、数组、Map 对象等。

## 2. 类和对象

### 2.1 定义类

```boyia
class Printer {
    fun say(msg) {
        BY_Log(msg);
    }
};
```

### 2.2 创建对象

Boyia 中通过原生函数 `new` 创建对象：

```boyia
var p = new(Printer);
p.say("hello");
```

## 3. 属性、方法与属性方法

### 3.1 普通方法

```boyia
class MathUtil {
    fun add(a, b) {
        return a + b;
    }
};
```

### 3.2 属性（prop 字段）

```boyia
class User {
    prop name = "guest";
};
```

### 3.3 属性方法（prop fun）

`prop` 可以和 `fun` 组合，定义属性方法：

```boyia
class Calc {
    prop fun mul(a, b) {
        return a * b;
    }
};
```

### 3.4 异步属性方法（prop async fun）

```boyia
class Service {
    prop async loadAsync(url) {
        var result = (await this.loadPromise(url));
        return result;
    }
};
```

## 4. 继承

使用 `extends` 实现继承：

```boyia
class BaseLogger {
    fun log(msg) {
        BY_Log(msg);
    }
};

class AppLogger extends BaseLogger {
    prop fun info(msg) {
        this.log("[INFO] " + msg);
    }
};
```

## 5. require 与模块加载

可使用 `require` 加载脚本文件：

```boyia
require("./util/util.boyia");
```

运行时会基于当前脚本路径解析相对路径并编译加载目标脚本。

## 6. Builtins 编写（Rust 侧）

Boyia 的 Builtins 通常通过创建全局类并挂载原生函数来实现。示例可参考 `examples/boyia_cli/src/builtins`。

常见步骤：

1. 在 Rust 中定义 builtin class（如 `File` / `Https` / `Zip`）。
2. 使用 `create_global_class` 创建类对象。
3. 用 `gen_builtin_class_function` 或同类注册逻辑挂载 native 方法。
4. 在方法中读取参数、执行逻辑、写回结果（同步或异步回调）。

异步 builtin（如文件/网络）常见模式：

- 在工作线程执行耗时任务。
- 切回 runtime 线程触发 Boyia 回调。
- 通过 `Map` 返回 `{status, data/message}`。

## 7. Rust 原生扩展函数（参照 boyia_lib）

`boyia_lib` 提供了标准 native 函数注册样例，例如：

- `create_object`（对应脚本 `new(...)`）
- `log_print`（对应脚本 `BY_Log(...)`）
- `require_file`（对应脚本 `require(...)`）

### 7.1 函数签名

native 函数通常形如：

```rust
pub unsafe fn your_native(vm: *mut LVoid) -> OpHandleResult
```

### 7.2 读取参数

通过 VM API 读取 local 参数：

- `get_local_size(vm)`：参数个数
- `get_local_value(index, vm)`：指定参数

### 7.3 注册到 Runtime

可参考 `boyia_runtime` 的 native 初始化逻辑，将函数加入 native 表：

```rust
self.append_native("yourFunc", your_native as NativePtr);
```

脚本侧即可直接调用：

```boyia
yourFunc(...);
```

## 8. async/await 机制

Boyia 已支持 `async/await` 语法，常用于异步 builtins 回调封装。

### 8.1 基本写法

```boyia
class Api {
    prop async loadAsync(url) {
        var result = (await this.loadPromise(url));
        return result;
    }

    prop async loadPromise(url) {
        Util.newMicrotask(fun(resolve) {
            Https.load(url, resolve);
        });
    }
};
```

### 8.2 使用建议

- 把 callback 形式的 builtin 包装成 `Promise` 风格函数（通过 `newMicrotask` + `resolve`）。
- 在业务函数里使用 `await`，让流程更直观。
- 统一异步返回结构（如 `status/data/message`）便于脚本侧判错。

## 9. 最小示例

```boyia
require("./util/util.boyia");

class Demo {
    prop fun run() {
        Util.log("Boyia start");
    }

    prop async runAsync(url) {
        var result = (await this.loadPromise(url));
        Util.log("result: " + result.get("status"));
    }

    prop async loadPromise(url) {
        Util.newMicrotask(fun(resolve) {
            Https.load(url, resolve);
        });
    }
};

var d = new(Demo);
d.run();
```

---

