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
    /// Resolved `require` paths discovered while compiling the file currently in `compile_string`.
    /// Consumed right after each `compile_string` to drive a post-order DFS over dependencies.
    pending_requires: Vec<String>,
    /// Rust CLI: when no [current_script], `BY_Require` resolves relative to this entry file.
    entry_script_path: String,
}

impl BoyiaCompileInfo {
    pub fn new() -> Self {
        Self {
            scripts: HashMap::new(),
            current_script: None,
            pending_requires: Vec::new(),
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

    /// Compile-time `require`: record an already-resolved dependency path for the file being compiled.
    /// Compilation is driven later by the post-order DFS in [compile_entry] / [compile_file].
    pub(crate) fn enqueue_script(&mut self, resolved_path: &str) {
        let key = Self::canonical_path(resolved_path);
        if self.scripts.contains_key(&key) || key == self.entry_script_path {
            return;
        }
        if self.pending_requires.iter().any(|p| *p == key) {
            return;
        }
        self.pending_requires.push(key);
    }

    /// Compile the entry source, then its `require` graph in post-order (dependencies first),
    /// leaving the entry's own global chain(s) last in `mEntry`.
    pub fn compile_entry(&mut self, script: &str, vm: &mut BoyiaVM, id_creator: &mut IdCreator) {
        let entry_start = vm.entry_len();
        // Entry has no `current_script`; its requires resolve relative to `entry_script_path`.
        self.compile_string(script, vm);
        let entry_end = vm.entry_len();

        let requires = std::mem::take(&mut self.pending_requires);
        for dep in requires {
            self.compile_file(&dep, vm, id_creator);
        }

        // Dependencies are now appended after the entry; move the entry's chain(s) to the end.
        vm.move_entries_to_end(entry_start, entry_end);
    }

    /// Compile `path` and its transitive `require`s in post-order (children before parent).
    /// After this returns, the file's own global chain(s) sit after all of its dependencies'.
    pub fn compile_file(&mut self, path: &str, vm: &mut BoyiaVM, id_creator: &mut IdCreator) {
        let dedup_key = Self::canonical_path(path);
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
        let entry_start = vm.entry_len();
        self.current_script = Some(Script::new(dedup_key.clone(), script_id, code_start));

        self.compile_string(&source, vm);

        let entry_end = vm.entry_len();
        let code_len = vm.vm_code().len();
        let code_end = if code_len > code_start as usize {
            (code_len - 1) as OpOffset
        } else {
            kInvalidInstruction
        };
        // Finalize and record this file before recursing, so cyclic requires terminate.
        if let Some(mut script) = self.current_script.take() {
            script.code_end = code_end;
            self.scripts.insert(dedup_key, script);
        }
        // Take this file's own requires; restore parent's compile context before recursing.
        let requires = std::mem::take(&mut self.pending_requires);
        self.current_script = saved_script;

        // DFS: compile dependencies (their chains append after this file's).
        for dep in requires {
            self.compile_file(&dep, vm, id_creator);
        }

        // Post-order: move this file's own chain(s) after its dependencies'.
        vm.move_entries_to_end(entry_start, entry_end);
    }
}

impl Default for BoyiaCompileInfo {
    fn default() -> Self {
        Self::new()
    }
}
