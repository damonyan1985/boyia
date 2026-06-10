//! Compile array literal edge cases; run with stderr visible to catch `syntax error` lines.

use boyia_runtime::BoyiaRuntime;

fn compile_snippet(source: &str) {
    let mut rt = BoyiaRuntime::create();
    rt.compile(source);
}

#[test]
fn compile_global_array_literals() {
    compile_snippet("var a = [];");
    compile_snippet("var b = [1];");
    compile_snippet("var c = [1, 2, 3];");
    compile_snippet("var d = [[1, 2], [3, 4]];");
    compile_snippet("var e = { items: [] };");
    compile_snippet("var f = { items: [1, 2] };");
    compile_snippet("var g = [\"789\", \"100\"];");
}

#[test]
fn compile_local_and_nested_array_literals() {
    compile_snippet(
        r#"
        fun demo() {
            var x = [];
            var y = [1, 2];
            var z = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
        }
    "#,
    );
}

#[test]
fn compile_array_in_call_args() {
    compile_snippet("fun f(a) {} fun g() { f([]); f([1]); f([1, 2]); f([[1, 2], [3]]); }");
}
