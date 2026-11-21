//! 统一日志入口：底层 crate 通过 [`Log`] 输出，避免散落的 `println!` / `eprintln!`。

use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

/// 日志级别，级别越高输出越详细：`None < Error < Warn < Info < Debug`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// 关闭全部日志。
    None,
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    const fn as_u8(self) -> u8 {
        match self {
            LogLevel::None => 0,
            LogLevel::Error => 1,
            LogLevel::Warn => 2,
            LogLevel::Info => 3,
            LogLevel::Debug => 4,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            0 => LogLevel::None,
            1 => LogLevel::Error,
            2 => LogLevel::Warn,
            3 => LogLevel::Info,
            _ => LogLevel::Debug,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            LogLevel::None => "none",
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(LogLevel::None),
            "error" => Some(LogLevel::Error),
            "warn" | "warning" => Some(LogLevel::Warn),
            "info" => Some(LogLevel::Info),
            "debug" => Some(LogLevel::Debug),
            _ => None,
        }
    }
}

/// 插件 cdylib 各自静态链接了一份 `boyia_types`，静态变量不共享；
/// 通过进程环境变量把级别同步给这些副本（它们在首次输出时读取）。
const ENV_LOG_LEVEL: &str = "BOYIA_LOG_LEVEL";
/// 尚未初始化（从环境变量兜底读取）。
const UNSET: u8 = u8::MAX;

/// 当前输出详细程度；未显式设置时由 `BOYIA_LOG_LEVEL` 决定，默认 `Debug`。
static MAX_LEVEL: AtomicU8 = AtomicU8::new(UNSET);

/// 日志输出结构体。
///
/// `Debug` / `Info` 写到 stdout，`Warn` / `Error` 写到 stderr。
pub struct Log;

impl Log {
    /// 设置输出详细程度：高于该级别的日志会被忽略，`None` 表示不输出任何日志。
    ///
    /// 同时写入 `BOYIA_LOG_LEVEL` 环境变量，使同进程内动态库中的 `Log` 副本生效；
    /// 因此应在加载插件前调用。
    pub fn set_level(level: LogLevel) {
        MAX_LEVEL.store(level.as_u8(), Ordering::Relaxed);
        std::env::set_var(ENV_LOG_LEVEL, level.as_str());
    }

    /// 当前输出详细程度。
    pub fn level() -> LogLevel {
        let value = MAX_LEVEL.load(Ordering::Relaxed);
        if value != UNSET {
            return LogLevel::from_u8(value);
        }
        let level = std::env::var(ENV_LOG_LEVEL)
            .ok()
            .and_then(|v| LogLevel::parse(&v))
            .unwrap_or(LogLevel::Debug);
        MAX_LEVEL.store(level.as_u8(), Ordering::Relaxed);
        level
    }

    /// 按级别输出格式化参数；高于当前级别或级别为 `None` 时不输出。
    pub fn log(level: LogLevel, args: fmt::Arguments<'_>) {
        if level > Self::level() {
            return;
        }
        match level {
            LogLevel::None => {}
            LogLevel::Debug | LogLevel::Info => println!("{args}"),
            LogLevel::Warn | LogLevel::Error => eprintln!("{args}"),
        }
    }

    pub fn debug(args: fmt::Arguments<'_>) {
        Self::log(LogLevel::Debug, args);
    }

    pub fn info(args: fmt::Arguments<'_>) {
        Self::log(LogLevel::Info, args);
    }

    pub fn warn(args: fmt::Arguments<'_>) {
        Self::log(LogLevel::Warn, args);
    }

    pub fn error(args: fmt::Arguments<'_>) {
        Self::log(LogLevel::Error, args);
    }
}
