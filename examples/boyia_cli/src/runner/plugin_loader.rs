//! Load cdylib plugins that export `boyia_plugin_registrars_ptr` / `_len`.

use boyia_plugin_sdk::layout::{default_plugin_dirs, ENV_ENABLE, ENV_PLUGIN_DIRS, ENV_STRICT};
use boyia_plugin_sdk::BuiltinRegistrar;
use boyia_plugin_sdk::BuiltinCtx;
use boyia_vm::BoyiaVM;
use libloading::{Library, Symbol};
use std::path::{Path, PathBuf};

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

pub struct PluginRegistry {
    _libraries: Vec<Library>,
    registrars: Vec<BuiltinRegistrar>,
}

impl PluginRegistry {
    pub fn registrars(&self) -> &[BuiltinRegistrar] {
        &self.registrars
    }

    pub fn load_from_dirs(dirs: &[PathBuf]) -> Result<Self, PluginLoadError> {
        let mut libraries = Vec::new();
        let mut registrars = Vec::new();

        for dir in dirs {
            if !dir.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if !is_plugin_dylib(&path) {
                    continue;
                }
                let loaded = load_one(&path)?;
                eprintln!(
                    "[plugin] loaded {:?} ({} registrar(s))",
                    path,
                    loaded.registrars.len()
                );
                registrars.extend(loaded.registrars);
                libraries.push(loaded.library);
            }
        }

        Ok(Self {
            _libraries: libraries,
            registrars,
        })
    }

    /// Each cdylib has its own `HOST_CTX_GETTER`; bind the CLI getter into every loaded plugin.
    pub fn bind_host_ctx(&self, getter: fn(&mut BoyiaVM) -> Option<BuiltinCtx>) {
        type BindFn = unsafe extern "C" fn(fn(&mut BoyiaVM) -> Option<BuiltinCtx>);
        for library in &self._libraries {
            let bind = match unsafe { library.get::<BindFn>(b"boyia_plugin_bind_host_ctx") } {
                Ok(b) => b,
                Err(_) => {
                    eprintln!("[plugin] warning: missing boyia_plugin_bind_host_ctx export");
                    continue;
                }
            };
            unsafe { bind(getter) };
        }
    }
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
    let library = unsafe { Library::new(path)? };

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
