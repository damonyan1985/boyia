//! Compile pipeline state: loaded scripts, current file path/id, string and file compile.
//! Port of `BoyiaCompileInfo` in `BoyiaTools/boyia-ide/boyia/src/lib/BoyiaRuntime.cpp`.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use crate::id_creator::IdCreator;
use boyia_vm::{compile_code, kInvalidInstruction, BoyiaVM, LInt, LUintPtr, OpOffset};
use std::collections::HashMap;
use std::ffi::CString;
use std::path::Path;

/// Source line/column for a compiled instruction.
pub(crate) struct SourcePosition {
    pub line_num: LInt,
    pub column_num: LInt,
}

/// One compiled script file and its instruction span in VM instruction table.
pub(crate) struct Script {
    /// Resolved path of the `.boyia` source file.
    pub script_path: String,
    /// Stable identifier from [`IdCreator::gen_ident_by_str`] for `script_path`.
    pub script_id: LUintPtr,
    /// First instruction offset ([`OpOffset`]) emitted for this file in VM code.
    pub code_start: OpOffset,
    /// Last instruction offset ([`OpOffset`]) emitted for this file in VM code (inclusive).
    pub code_end: OpOffset,
    /// Source position per instruction offset within this script's span.
    pub code_positions: HashMap<OpOffset, SourcePosition>,
}

impl Script {
    pub(crate) fn new(script_path: String, script_id: LUintPtr, code_start: OpOffset) -> Self {
        Self {
            script_path,
            script_id,
            code_start,
            code_end: kInvalidInstruction,
            code_positions: HashMap::new(),
        }
    }

    pub(crate) fn set_code_position(&mut self, code_index: OpOffset, line_num: LInt, column_num: LInt) {
        self.code_positions.insert(
            code_index,
            SourcePosition {
                line_num,
                column_num,
            },
        );
    }
}

/// Mirrors C++ `BoyiaCompileInfo` (`m_programSet`, `m_currentScriptPath`, `m_currentScriptId`, `compile` / `compileFile`).
pub(crate) struct BoyiaCompileInfo {
    /// Finished scripts keyed by canonical path.
    scripts: HashMap<String, Script>,
    /// Script currently being compiled (`compile_file`); moved into [scripts] when done.
    current_script: Option<Script>,
    /// Scripts discovered via compile-time `require`, not yet compiled (FIFO).
    pending_scripts: Vec<Script>,
    /// Rust CLI: when no [current_script], `BY_Require` resolves relative to this entry file.
    entry_script_path: String,
}

impl BoyiaCompileInfo {
    pub fn new() -> Self {
        Self {
            scripts: HashMap::new(),
            current_script: None,
            pending_scripts: Vec::new(),
            entry_script_path: String::new(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn current_script_path(&self) -> &str {
        self.current_script
            .as_ref()
            .map(|script| script.script_path.as_str())
            .unwrap_or("")
    }

    #[allow(dead_code)]
    pub(crate) fn current_script_id(&self) -> LUintPtr {
        self.current_script
            .as_ref()
            .map(|script| script.script_id)
            .unwrap_or(0)
    }

    /// Persist main script path for relative requires (see `BoyiaRuntime::set_entry_script_path`).
    pub fn set_entry_script_path(&mut self, path: &str) {
        let p = Path::new(path);
        self.entry_script_path = std::fs::canonicalize(p)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
    }

    /// C++ `BoyiaCompileInfo::compile` → `CompileCode(script, vm)`.
    pub fn compile_string(&self, script: &str, vm: &mut BoyiaVM) {
        let script_c = CString::new(script).unwrap_or_default();
        unsafe {
            compile_code(script_c.as_ptr() as *mut _, vm);
        }
    }

    /// Path context for `BY_Require`: active `compile_file` target, else `entry_script_path`, else empty (caller may use CWD).
    pub fn require_path_base(&self) -> &str {
        if let Some(script) = &self.current_script {
            return &script.script_path;
        }
        if !self.entry_script_path.is_empty() {
            return &self.entry_script_path;
        }
        ""
    }

    /// `SetCodePosition` while compiling the active script file.
    pub(crate) fn set_code_position(&mut self, code_index: OpOffset, line_num: LInt, column_num: LInt) {
        let Some(script) = &mut self.current_script else {
            return;
        };
        script.set_code_position(code_index, line_num, column_num);
    }

    fn canonical_path(path: &str) -> String {
        std::fs::canonicalize(Path::new(path))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string())
    }

    /// True when `path` is already compiled, pending, currently compiling, or the entry.
    fn is_script_known(&self, path: &str) -> bool {
        self.scripts.contains_key(path)
            || self.entry_script_path == path
            || self.pending_scripts.iter().any(|s| s.script_path == path)
            || self
                .current_script
                .as_ref()
                .is_some_and(|s| s.script_path == path)
    }

    /// Compile-time `require`: enqueue an already-resolved script path for later compilation.
    /// Does not compile or execute now (see [drain_pending_scripts]).
    pub(crate) fn enqueue_script(&mut self, resolved_path: &str, id_creator: &mut IdCreator) {
        let dedup_key = Self::canonical_path(resolved_path);
        if self.is_script_known(&dedup_key) {
            return;
        }
        let script_id = id_creator.gen_ident_by_str(&dedup_key);
        self.pending_scripts
            .push(Script::new(dedup_key, script_id, kInvalidInstruction));
    }

    /// Compile all queued scripts (FIFO). Each compile may enqueue more requires.
    pub(crate) fn drain_pending_scripts(&mut self, vm: &mut BoyiaVM, id_creator: &mut IdCreator) {
        while !self.pending_scripts.is_empty() {
            let script = self.pending_scripts.remove(0);
            if self.scripts.contains_key(&script.script_path) {
                continue;
            }
            self.compile_file(&script.script_path, vm, id_creator);
        }
    }

    /// C++ `BoyiaCompileInfo::compileFile`: skip if path seen, read file, `compile`, restore previous path/id.
    pub fn compile_file(&mut self, path: &str, vm: &mut BoyiaVM, id_creator: &mut IdCreator) {
        let dedup_key = std::fs::canonicalize(Path::new(path))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());

        if self.scripts.contains_key(&dedup_key) {
            return;
        }

        let saved_script = self.current_script.take();

        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("compile_file: read {}: {}", path, e);
                self.current_script = saved_script;
                return;
            }
        };

        if source.is_empty() {
            self.current_script = saved_script;
            return;
        }

        let script_id = id_creator.gen_ident_by_str(&dedup_key);
        let code_start = vm.vm_code().len() as OpOffset;
        self.current_script = Some(Script::new(dedup_key.clone(), script_id, code_start));

        self.compile_string(&source, vm);

        let code_len = vm.vm_code().len();
        let code_end = if code_len > code_start as usize {
            (code_len - 1) as OpOffset
        } else {
            kInvalidInstruction
        };
        if let Some(script) = self.current_script.as_mut() {
            script.code_end = code_end;
        }
        if let Some(script) = self.current_script.take() {
            self.scripts.insert(dedup_key, script);
        }

        self.current_script = saved_script;
    }
}

impl Default for BoyiaCompileInfo {
    fn default() -> Self {
        Self::new()
    }
}
