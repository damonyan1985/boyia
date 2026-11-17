//! Load cdylib plugins that export `boyia_plugin_registrars_ptr` / `_len`.

use crate::layout::{default_plugin_dirs, ENV_ENABLE, ENV_PLUGIN_DIRS, ENV_STRICT};
use crate::{BuiltinCtx, BuiltinRegistrar};
use boyia_vm::{BoyiaValue, BoyiaVM, LUintPtr};
use libloading::{Library, Symbol};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug)]
pub enum PluginLoadError {
    Io(std::io::Error),
    LibLoad(libloading::Error),
    MissingSymbol { path: PathBuf, symbol: String },
}

impl std::fmt::Display for PluginLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "plugin io error: {e}"),
            Self::LibLoad(e) => write!(f, "plugin load error: {e}"),
            Self::MissingSymbol { path, symbol } => {
                write!(f, "plugin {:?} missing symbol {symbol}", path)
            }
        }
    }
}

impl From<std::io::Error> for PluginLoadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<libloading::Error> for PluginLoadError {
    fn from(e: libloading::Error) -> Self {
        Self::LibLoad(e)
    }
}

pub fn dylib_builtins_enabled() -> bool {
    match std::env::var(ENV_ENABLE).ok().as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => default_plugin_dirs().iter().any(|dir| dir_has_dylib(dir)),
    }
}

fn dir_has_dylib(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(|e| e.ok()).any(|e| is_plugin_dylib(&e.path()))
}

pub fn plugin_strict() -> bool {
    std::env::var(ENV_STRICT).ok().as_deref() == Some("1")
}

pub fn plugin_dirs_from_env() -> Vec<PathBuf> {
    std::env::var(ENV_PLUGIN_DIRS)
        .ok()
        .map(|s| {
            s.split([';', ':'])
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_else(default_plugin_dirs)
}

struct LoadedOne {
    library: Library,
    registrars: Vec<BuiltinRegistrar>,
}

fn is_plugin_dylib(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e, "dll" | "so" | "dylib"))
        .unwrap_or(false)
}

fn load_one(path: &Path) -> Result<LoadedOne, PluginLoadError> {
    let library = open_plugin(path)?;
    load_from_library(path, library)
}

/// 用系统加载器打开插件，并尽量使用各平台的懒加载/搜索策略。
fn open_plugin(path: &Path) -> Result<Library, PluginLoadError> {
    #[cfg(windows)]
    {
        // LOAD_WITH_ALTERED_SEARCH_PATH：优先在插件所在目录解析其依赖，
        // 避免 LoadLibrary 在 PATH / 当前目录中逐项探测。
        let lib = unsafe {
            libloading::os::windows::Library::load_with_flags(
                path,
                libloading::os::windows::LOAD_WITH_ALTERED_SEARCH_PATH,
            )?
        };
        Ok(lib.into())
    }
    #[cfg(unix)]
    {
        // RTLD_LAZY：PLT 符号绑定推迟到首次调用；RTLD_LOCAL：不污染全局符号空间。
        let lib = unsafe {
            libloading::os::unix::Library::open(
                Some(path),
                libloading::os::unix::RTLD_LAZY | libloading::os::unix::RTLD_LOCAL,
            )?
        };
        Ok(lib.into())
    }
    #[cfg(not(any(windows, unix)))]
    {
        Ok(unsafe { Library::new(path)? })
    }
}

/// 从已打开的库读取 registrar 表。
fn load_from_library(path: &Path, library: Library) -> Result<LoadedOne, PluginLoadError> {
    let ptr_fn: Symbol<unsafe extern "C" fn() -> *const BuiltinRegistrar> = unsafe {
        library
            .get(b"boyia_plugin_registrars_ptr")
            .map_err(|_| PluginLoadError::MissingSymbol {
                path: path.to_path_buf(),
                symbol: "boyia_plugin_registrars_ptr".into(),
            })?
    };
    let len_fn: Symbol<unsafe extern "C" fn() -> usize> = unsafe {
        library
            .get(b"boyia_plugin_registrars_len")
            .map_err(|_| PluginLoadError::MissingSymbol {
                path: path.to_path_buf(),
                symbol: "boyia_plugin_registrars_len".into(),
            })?
    };

    if let Ok(name_fn) = unsafe { library.get::<unsafe extern "C" fn() -> *const i8>(b"boyia_plugin_name") }
    {
        let name_ptr = unsafe { name_fn() };
        if !name_ptr.is_null() {
            let name = unsafe { std::ffi::CStr::from_ptr(name_ptr) };
            eprintln!("[plugin] name={}", name.to_string_lossy());
        }
    }

    let ptr = unsafe { ptr_fn() };
    let len = unsafe { len_fn() };
    if ptr.is_null() || len == 0 {
        return Ok(LoadedOne {
            library,
            registrars: vec![],
        });
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    Ok(LoadedOne {
        library,
        registrars: slice.to_vec(),
    })
}

fn bind_library_host_ctx(library: &Library, getter: fn(&mut BoyiaVM) -> Option<BuiltinCtx>) {
    type BindFn = unsafe extern "C" fn(fn(&mut BoyiaVM) -> Option<BuiltinCtx>);
    match unsafe { library.get::<BindFn>(b"boyia_plugin_bind_host_ctx") } {
        Ok(bind) => unsafe { bind(getter) },
        Err(_) => eprintln!("[plugin] warning: missing boyia_plugin_bind_host_ctx export"),
    }
}

/// 与启动期注册完全一致：在宿主 VM 线程上执行 registrar。
fn register_registrars(vm: &mut BoyiaVM, registrars: &[BuiltinRegistrar]) {
    if registrars.is_empty() {
        return;
    }
    let rt = unsafe { boyia_vm::get_runtime_from_vm(vm) };
    if rt.is_null() {
        eprintln!("[plugin] warning: VM has no runtime; skip builtin registration");
        return;
    }
    let mut gen_id = |s: &str| unsafe { (*rt).gen_identifier(s) };
    for register in registrars {
        register(vm, &mut gen_id);
    }
}

// ---------------------------------------------------------------------------
// 懒加载：启动仅建立“类名 → dll”索引（零 dlopen），首次调用时单次加载
// ---------------------------------------------------------------------------

struct LazyState {
    /// 类名 → 插件 dll（来自 `<dll>.plugin.json`）。
    routes: HashMap<String, PathBuf>,
    /// 没有清单的 dll：无法预路由，仅在兜底时批量加载一次。
    unindexed: Vec<PathBuf>,
    /// 已加载（或已尝试加载失败）的 dll，避免重复 dlopen。
    loaded: Vec<PathBuf>,
    fallback_done: bool,
    /// 已加载库必须常驻：registrar 是 dll 内的函数指针。
    _libraries: Vec<Library>,
}

/// 启动期零 dlopen 的插件注册表：
/// 1. 扫描目录并读取 `*.plugin.json` 建立类名路由；
/// 2. 脚本首次访问插件类时，只 dlopen 该类归属的 dll 一次并注册；
/// 3. 注册后直接返回查表结果，不做任何重试循环。
pub struct LazyPluginLoader {
    state: Mutex<LazyState>,
    strict: bool,
    host_getter: OnceLock<fn(&mut BoyiaVM) -> Option<BuiltinCtx>>,
}

impl LazyPluginLoader {
    pub fn new(dirs: &[PathBuf], strict: bool) -> Self {
        let mut paths = Vec::new();
        for dir in dirs {
            if !dir.is_dir() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if is_plugin_dylib(&path) {
                    paths.push(path);
                }
            }
        }
        paths.sort();

        let mut routes = HashMap::new();
        let mut unindexed = Vec::new();
        for path in &paths {
            let classes = read_manifest_classes(path);
            if classes.is_empty() {
                unindexed.push(path.clone());
                continue;
            }
            for class in classes {
                routes.entry(class).or_insert_with(|| path.clone());
            }
        }
        eprintln!(
            "[plugin] indexed {} dylib(s), {} class route(s), {} without manifest (no dlopen)",
            paths.len(),
            routes.len(),
            unindexed.len()
        );
        Self {
            state: Mutex::new(LazyState {
                routes,
                unindexed,
                loaded: Vec::new(),
                fallback_done: false,
                _libraries: Vec::new(),
            }),
            strict,
            host_getter: OnceLock::new(),
        }
    }

    /// 与 [crate::set_host_ctx_getter] 相同；每个 dll 加载后立即绑定。
    pub fn set_host_ctx_getter(&self, getter: fn(&mut BoyiaVM) -> Option<BuiltinCtx>) {
        let _ = self.host_getter.set(getter);
    }

    /// 首次访问全局类 `key` 时：O(1) 路由 → 单次 dlopen → 返回全局值。
    fn resolve(&self, vm: &mut BoyiaVM, key: LUintPtr) -> *mut BoyiaValue {
        let Some(name) = (unsafe { boyia_vm::name_for_identifier(vm, key) }) else {
            return std::ptr::null_mut();
        };

        let (route, run_fallback) = {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let route = state
                .routes
                .get(&name)
                .filter(|p| !state.loaded.contains(*p))
                .cloned();
            let run_fallback =
                route.is_none() && !state.fallback_done && !state.unindexed.is_empty();
            (route, run_fallback)
        };

        let mut routed = false;
        if let Some(path) = route {
            routed = self.load_and_register(vm, &path);
        } else if run_fallback {
            self.load_unindexed(vm);
        }

        let mut value = unsafe { boyia_vm::find_global_now(vm, key) };
        if value.is_null() && routed {
            // 清单与实际注册不一致时，兜底加载无清单插件一次。
            self.load_unindexed(vm);
            value = unsafe { boyia_vm::find_global_now(vm, key) };
        }
        value
    }

    /// 加载一个 dll 并注册其 builtins。每个 dll 至多尝试一次。
    fn load_and_register(&self, vm: &mut BoyiaVM, path: &Path) -> bool {
        let loaded = match load_one(path) {
            Ok(loaded) => loaded,
            Err(err) => {
                if self.strict {
                    eprintln!("Error: plugin load failed: {err}");
                    std::process::exit(1);
                }
                eprintln!("Warning: plugin load failed: {err}");
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                state.loaded.push(path.to_path_buf());
                return false;
            }
        };

        if let Some(getter) = self.host_getter.get().copied() {
            bind_library_host_ctx(&loaded.library, getter);
        }
        register_registrars(vm, &loaded.registrars);
        eprintln!(
            "[plugin] lazy-loaded {:?} ({} registrar(s))",
            path,
            loaded.registrars.len()
        );

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.loaded.push(path.to_path_buf());
        state._libraries.push(loaded.library);
        true
    }

    /// 无清单插件兜底：一次性加载全部（每进程至多一次）。
    fn load_unindexed(&self, vm: &mut BoyiaVM) {
        let paths = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.fallback_done {
                return;
            }
            state.fallback_done = true;
            std::mem::take(&mut state.unindexed)
        };
        for path in paths {
            let already = self
                .state
                .lock()
                .map(|s| s.loaded.contains(&path))
                .unwrap_or(true);
            if !already {
                self.load_and_register(vm, &path);
            }
        }
    }
}

/// 读取 `<dll>.plugin.json`（`{"classes":["ZipPlugin"]}`），不加载 dll。
fn read_manifest_classes(dll: &Path) -> Vec<String> {
    let manifest = dll.with_extension("plugin.json");
    let Ok(bytes) = std::fs::read(&manifest) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        eprintln!("[plugin] warning: invalid manifest {:?}", manifest);
        return Vec::new();
    };
    value
        .get("classes")
        .and_then(|v| v.as_array())
        .map(|classes| {
            classes
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

static LAZY_REGISTRY: OnceLock<Arc<LazyPluginLoader>> = OnceLock::new();
static LAZY_LOADING: AtomicBool = AtomicBool::new(false);

/// 安装进程级懒加载注册表并挂接到 `boyia_vm` 全局查找。每进程只生效一次。
pub fn install_lazy_plugin_hook(registry: Arc<LazyPluginLoader>) -> bool {
    if LAZY_REGISTRY.set(registry).is_err() {
        return false;
    }
    boyia_vm::set_global_miss_hook(lazy_global_miss_hook)
}

fn lazy_global_miss_hook(vm: &mut BoyiaVM, key: LUintPtr) -> *mut BoyiaValue {
    // 重入保护：注册过程若再次触发全局查找，不得再进入加载。
    if LAZY_LOADING.swap(true, Ordering::SeqCst) {
        return std::ptr::null_mut();
    }
    let value = match LAZY_REGISTRY.get() {
        Some(registry) => registry.resolve(vm, key),
        None => std::ptr::null_mut(),
    };
    LAZY_LOADING.store(false, Ordering::SeqCst);
    value
}
