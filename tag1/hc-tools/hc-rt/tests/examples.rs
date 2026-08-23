//! 解释器集成测试：以 examples 的 [test] fn 为验收基线（Q-T2/Q-T6）
//!
//! 覆盖 tag1 垂直切片核心功能：变量/控制流/类型/函数/错误/class/enum/
//! 闭包/切片/序列化/集合/所有权/泛型。

use hc_rt::Interp;
use std::thread;

/// 运行单个 .hc 文件的所有 test fn，返回 (passed, failed, skipped)。
/// 在 64MB 栈线程中运行（tree-walking 求值递归栈深，镜像 CLI 64MB 做法；
/// 否则深递归/大帧在默认测试线程栈上溢出，如 ex46_recursion）。
fn run_tests_in(path: &str) -> (usize, usize, usize) {
    let path = path.to_string();
    thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let src =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
            let program =
                hc::parse_source(&src).unwrap_or_else(|d| panic!("parse {path}: {:?}", d));
            let mut interp = Interp::new(&src);
            interp
                .load(&program)
                .unwrap_or_else(|e| panic!("load {path}: {}", e.name));
            let r = interp.run_tests();
            for line in &interp.test_out {
                if line.starts_with("[FAIL]") {
                    eprintln!("{path}::{line}");
                }
            }
            r
        })
        .expect("spawn example test thread")
        .join()
        .expect("example test thread panicked")
}

// 相对 workspace 解析（`<repo>/tag1/hc-rt` → 上两级 → `<repo>/examples`），
// 避免硬编码绝对路径——CI（Linux）与本地（Windows）均可运行。
const EXAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");

#[test]
fn ex01_hello() {
    let (p, f, s) = run_tests_in(&format!("{EXAMPLES}/01-syntax/01-basic/01-hello.hc"));
    assert_eq!(f, 0, "failed={f} skipped={s}");
    assert!(p >= 1);
}

#[test]
fn ex02_variables() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/01-basic/02-variables.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex03_control_flow() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/01-basic/03-control-flow.hc"));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex04_ranges() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/01-basic/04-ranges.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex06_integers() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/06-integers.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex08_bool_void() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/08-bool-void.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex09_arrays() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/09-arrays.hc"));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex12_bitops() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/12-bitops.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex13_struct() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/13-struct.hc"));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex14_enum() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/14-enum.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex15_pointers() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/15-pointers.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex16_slices() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/16-slices.hc"));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex17_optional() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/17-optional.hc"));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex19_nested_data() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/19-nested-data.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex21_closures() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/03-functions/21-closures.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex23_tests() {
    let (p, f, s) = run_tests_in(&format!("{EXAMPLES}/01-syntax/03-functions/23-tests.hc"));
    assert_eq!(f, 0);
    // F1：skip_example 触发 error.SkipTest → 统计为 SKIP（s >= 1），其余 5 项通过
    assert!(p >= 5);
    assert!(s >= 1);
}

#[test]
fn ex27_ownership() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/04-memory/27-ownership.hc"));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex29_globals() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/04-memory/29-globals.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex30_interface() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/05-oop/30-interface.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex31_class() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/05-oop/31-class.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex32_collections() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/06-data/32-collections.hc"));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex34_generics() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/07-meta/34-generics.hc"));
    assert!(f <= 1, "anytype 通过，comptime 类型应用（E1）可失败");
    assert!(p >= 1);
}

#[test]
fn ex45_strings() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/45-strings.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex46_recursion() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/46-recursion.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex47_sort_search() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/47-sort-search.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex50_serialization() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/50-serialization.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex51_collection_bytes() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/51-collection-bytes.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex52_string_deep() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/52-string-deep.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex53_map_deep() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/53-map-deep.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex56_csv_parse() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/56-csv-parse.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex58_copy_semantics() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/58-copy-semantics.hc"));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex59_pipeline() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/59-pipeline.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex61_json_walk() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/61-json-walk.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex62_custom_sort() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/62-custom-sort.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex63_template_render() {
    // D1：fmt_int 格式辅助——63-template-render 占位符替换（从 E1 失败池移出）
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/02-idioms/63-template-render.hc"));
    assert_eq!(f, 0, "fmt_int 缺失时 failed={f}");
    assert!(p >= 1);
}

#[test]
fn ex64_interface_poly() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/03-patterns/64-interface-poly.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex66_builder_chain() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/03-patterns/66-builder-chain.hc"));
    assert_eq!(f, 0);
    assert!(p >= 1);
}

#[test]
fn ex71_recursive_parser() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/03-patterns/71-recursive-parser.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex84_rng() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/05-tools/84-rng.hc"));
    assert_eq!(f, 0);
    assert!(p >= 2);
}

#[test]
fn ex86_scalar_interfaces() {
    let (p, f, _) = run_tests_in(&format!(
        "{EXAMPLES}/01-syntax/02-types/86-scalar-interfaces.hc"
    ));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex87_overloads() {
    let (p, f, _) = run_tests_in(&format!(
        "{EXAMPLES}/01-syntax/03-functions/87-overloads.hc"
    ));
    // 期望类型传播（M2）未实现：return_type_overload 依赖目标类型选择重载
    assert!(f <= 1, "重载解析（具体优先泛型）已实现；返回类型选择留 M2");
    assert!(p >= 3);
}

#[test]
fn ex88_iterators() {
    let (p, f, _) = run_tests_in(&format!("{EXAMPLES}/01-syntax/02-types/88-iterators.hc"));
    assert_eq!(f, 0);
    assert!(p >= 3);
}

#[test]
fn ex90_thread_lifecycle() {
    // 组 G 线程生命周期（G5）：spawn/join/cancel/is_done/detach + Q8 每线程 alloc
    let (p, f, _) = run_tests_in(&format!(
        "{EXAMPLES}/04-concurrency/90-thread-lifecycle.hc"
    ));
    assert_eq!(f, 0);
    assert!(p >= 5);
}
