//! CLI argument parsing and `.boyia_rc` dev configuration.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const RC_FILE_NAME: &str = ".boyia_rc";

/// Keys accepted in `.boyia_rc` for the entry script path.
const RC_SCRIPT_KEYS: &[&str] = &["script", "entry", "main"];

/// Resolve the Boyia entry script.
///
/// 1. If a CLI path is given, use it when the file exists.
/// 2. Otherwise fall back to the first `.boyia_rc` (project root → exe dir → home).
pub fn resolve_entry_script(cli_arg: Option<&str>) -> Result<PathBuf, String> {
    if let Some(arg) = cli_arg {
        if !arg.is_empty() {
            if let Ok(path) = resolve_user_path(Path::new(arg)) {
                return Ok(path);
            }
        }
    }

    resolve_entry_script_from_rc()
}

fn resolve_entry_script_from_rc() -> Result<PathBuf, String> {
    for rc_path in boyia_rc_search_paths() {
        if !rc_path.is_file() {
            continue;
        }
        let content = fs::read_to_string(&rc_path).map_err(|e| {
            format!("failed to read {}: {e}", rc_path.display())
        })?;
        let script_value = parse_boyia_rc(&content).map_err(|e| {
            format!("{}: {e}", rc_path.display())
        })?;
        let script_path = resolve_rc_script_path(&rc_path, &script_value);
        return resolve_user_path(&script_path).map_err(|e| {
            format!("from {}: {e}", rc_path.display())
        });
    }

    Err(format!(
        "no script path: pass an existing .boyia file as argument, or create {RC_FILE_NAME} in the \
         project root, next to the executable, or in your home directory"
    ))
}

/// Parse `argv` (program name stripped). Returns `None` for help flags.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<String>, String> {
    let mut paths = Vec::new();
    for arg in args {
        if arg == "-h" || arg == "--help" {
            return Ok(None);
        }
        if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        }
        paths.push(arg);
    }
    match paths.len() {
        0 => Ok(Some(String::new())), // signal: use .boyia_rc
        1 => Ok(Some(paths.into_iter().next().unwrap())),
        n => Err(format!("expected at most one script path, got {n}")),
    }
}

pub fn print_usage(program: &str) {
    println!(
        "\
Boyia CLI — compile and run Boyia scripts

Usage:
  {program} [script.boyia]

Script resolution:
  1. If a path is given on the command line and the file exists, use it.
  2. Otherwise read the first existing {RC_FILE_NAME}:
     a. <project-root>/.boyia_rc  (nearest ancestor of cwd with Cargo.toml or .git)
     b. <exe-dir>/.boyia_rc       (directory containing this binary)
     c. ~/.boyia_rc               (user home, e.g. /Users/<you>)

{RC_FILE_NAME} format (UTF-8 text, # comments, blank lines ignored):
  script=path/to/main.boyia

Relative paths in {RC_FILE_NAME} are resolved against that file's directory.
Environment:
  BOYIA_INIT_MINIMAL=1   skip registering CLI builtin classes (faster init)
"
    );
}

fn boyia_rc_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(root) = find_project_root() {
        paths.push(root.join(RC_FILE_NAME));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join(RC_FILE_NAME));
        }
    }
    if let Some(home) = home_dir() {
        paths.push(home.join(RC_FILE_NAME));
    }
    paths
}

/// Nearest ancestor of the current directory that looks like a project root.
fn find_project_root() -> Option<PathBuf> {
    let mut dir = env::current_dir().ok()?;
    loop {
        if dir.join("Cargo.toml").is_file() || dir.join(".git").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

/// Read `script` / `entry` / `main` from `.boyia_rc` content.
pub fn parse_boyia_rc(content: &str) -> Result<String, String> {
    for line in content.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if RC_SCRIPT_KEYS.contains(&key.trim()) {
            let value = unquote(value.trim());
            if !value.is_empty() {
                return Ok(value);
            }
        }
    }
    Err(format!(
        "missing script entry (expected one of: {})",
        RC_SCRIPT_KEYS.join(", ")
    ))
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn resolve_rc_script_path(rc_path: &Path, script_value: &str) -> PathBuf {
    let script = PathBuf::from(script_value);
    if script.is_absolute() {
        script
    } else {
        rc_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(script)
    }
}

fn resolve_user_path(path: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    if !path.is_file() {
        return Err(format!("script not found: {}", path.display()));
    }
    path.canonicalize()
        .map_err(|e| format!("script not found: {} ({e})", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rc_script_key() {
        let content = "# dev\nscript = script/main.boyia\n";
        assert_eq!(parse_boyia_rc(content).unwrap(), "script/main.boyia");
    }

    #[test]
    fn parse_rc_quoted() {
        let content = r#"entry="my/app.boyia""#;
        assert_eq!(parse_boyia_rc(content).unwrap(), "my/app.boyia");
    }

    #[test]
    fn resolve_rc_relative() {
        let rc = Path::new("/proj/.boyia_rc");
        let got = resolve_rc_script_path(rc, "script/main.boyia");
        assert_eq!(got, Path::new("/proj/script/main.boyia"));
    }

    #[test]
    fn find_project_root_has_cargo_toml() {
        let root = find_project_root().expect("project root from test cwd");
        assert!(root.join("Cargo.toml").is_file());
    }

    #[test]
    fn cli_missing_path_falls_back_to_rc() {
        let rc_dir = std::env::current_dir().expect("cwd");
        let rc_path = rc_dir.join(RC_FILE_NAME);
        if !rc_path.is_file() {
            return;
        }
        let content = fs::read_to_string(&rc_path).expect("read .boyia_rc");
        let script_value = parse_boyia_rc(&content).expect("parse .boyia_rc");
        let expected = resolve_user_path(&resolve_rc_script_path(&rc_path, &script_value))
            .expect("rc script exists");

        let got = resolve_entry_script(Some("definitely_missing_script.boyia"))
            .expect("fallback to .boyia_rc");
        assert_eq!(got, expected);
    }
}
