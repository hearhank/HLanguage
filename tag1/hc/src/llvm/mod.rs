//! M3.3 LLVM 原生后端（emit-.ll 文本 + 外部 `zig cc` 驱动）
//!
//! 与 M3.1 IR 参考解释器（`ir::run_ir`）共用 `IrModule`（ADR-0004 唯一语义源），
//! 逐条对齐 `exec_func` 的动态语义。IR 槽是无类型的 `IrValue`，首轮用**统一带标签
//! 值表示** `%Value = { i32 tag, i128 data }`（正确性优先），动态运算集中到导言 helper，
//! 避免每个 `Bin` 内联 tag-dispatch。i128 载荷修复 i64 截断；浮点位模式存低 64 位。
//!
//! 覆盖（tag1 切片）：标量 / 控制流 / 函数调用 / 错误值通道 / 断言内建
//! + Phase 1 指针（`&`/`&mut` 取址 → `%Value` tag 7 载荷 = 槽地址；`p.*` 解引用；
//! 写穿经 `hc_store_ptr`）。指针比较：同指针身份相等（`hc_eq`），
//! Ptr 与普通值比较时先解引用（对齐 `IrValue::value_eq`）；`<` 仅 Ptr/Ptr 按地址序。
//! + Phase 2 聚合（数组/切片/class/enum 堆对象 + 16 helper：字段/索引/切片读写、
//! 字面量构造、解构、move、unwrap、深比较）+
//! Phase 3 switch + range + for（`hc_match_test` 模式描述符分发、`hc_make_range`、
//! `%IterObj/%IterItemObj` + `hc_iter_*`；Mut/Move 捕获 = copy-in/copy-out 写回）。
//! 已知简化见 07-bootstrap-plan.md：NUL 结尾字符串字面量、
//! 无优化 pass、硬错误消息依赖 libc `puts`/`exit`。

mod body;
mod emit;
mod helpers;
mod preamble;
#[cfg(test)]
mod tests;
pub(crate) mod text;

use crate::errorcodes::ErrorCodeTable;
use crate::ir::{IrConst, IrInst, IrModule, IrPattern};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

pub(crate) use self::text::*;

use self::emit::*;
use self::preamble::*;

// ---------- 值 tag 常量（与 IrValue 动态分派对应） ----------

const T_VOID: i32 = 0;
const T_NULL: i32 = 1;
const T_INT: i32 = 2;
const T_FLOAT: i32 = 3;
const T_BOOL: i32 = 4;
const T_STR: i32 = 5;
const T_ERR: i32 = 6;
const T_PTR: i32 = 7;
// ---- Phase 2 聚合 ----
const T_ARR: i32 = 8;
const T_SLICE: i32 = 9;
const T_CLASS: i32 = 10;
const T_ENUM: i32 = 11;
const T_END: i32 = 12;
// ---- Phase 3 迭代器（tag 13 直接内联在 `hc_iter_make` 的 IR 文本中） ----
// ---- Phase 4 函数引用 / 闭包（tag 14/15；原生 ABI 工作留待 Phase 7，当前经 hc_abort 拒绝） ----
// Phase 8 原生 ABI 落地启用
const T_FN: i32 = 14;
// Phase 8 原生 ABI 落地启用
const T_CLOSURE: i32 = 15;

/// 生成完整 `.ll` 模块文本（导言 + 每个 `IrFunc` + `main` 包装）。
pub fn codegen(module: &IrModule, errors: &ErrorCodeTable) -> String {
    codegen_inner(module, errors, &HashMap::new(), "", true)
}

/// C3：exe 链接本地库形态——codegen 时把未登记限定名（canon miss 含 `.`）路由到
/// 外部链接符号（`{pkg}.hc_fn{i}`，依赖库 .sym 表）。未命中仍响亮中止。
pub fn codegen_with_links(
    module: &IrModule,
    errors: &ErrorCodeTable,
    links: &HashMap<String, String>,
) -> String {
    codegen_inner(module, errors, links, "", true)
}

/// C3：库形态运行时声明化——模板 helper/基建（`define ... @hc_...`）转 `declare`、
/// `@hc_fail_msg` 全局转 external；用户函数（`define %Value @"{pkg}.hc_fn{i}"`）保留
/// define。链接时由 exe 提供运行时定义——库 .o 与 exe .o 无重复符号。
fn runtime_to_declares(ll: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for line in ll.lines() {
        if skip {
            if line.trim() == "}" {
                skip = false;
            }
            continue;
        }
        let t = line.trim();
        if t.starts_with("define ") && t.contains("@hc_") && !t.starts_with("define %Value @\"") {
            let sig = t
                .trim_start_matches("define ")
                .trim_end_matches('{')
                .trim_end();
            out.push_str("declare ");
            out.push_str(sig);
            out.push('\n');
            skip = true;
        } else if t.starts_with("@hc_fail_msg = global") {
            out.push_str("@hc_fail_msg = external global i8*\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// C3/C4：库形态 codegen——函数/内部调用名带包前缀（`@{pkg}.hc_fn{i}`，跨模块链接唯一）、
/// 跳过全局表（`@.h_globals` 跨 .o 撞符号——库全局链接留后续）与 main wrapper。
/// `dll_mode`：静态归档（false）→ 运行时 helper/基建转 declare（由链接的 exe 提供定义）；
/// dll（true）→ **自包含**（helper 保持 define——运行时加载的 dll 无法依赖 exe 提供符号）。
pub fn codegen_lib(
    module: &IrModule,
    errors: &ErrorCodeTable,
    pkg: &str,
    dll_mode: bool,
) -> String {
    let strings = collect_strings(module);
    let canon: HashMap<String, Vec<usize>> = module
        .func_index
        .iter()
        .map(|(name, idxs)| (name.clone(), idxs.clone()))
        .collect();
    let gidx = globals_index(module);
    let closure_caps = closure_caps_map(module);
    let mut out = String::new();
    let mut ext_decls: Vec<(String, usize)> = Vec::new();
    let pfx = &format!("{pkg}.");
    emit_preamble(&mut out, &strings, &module.continuous, true);
    for (idx, f) in module.funcs.iter().enumerate() {
        emit_func(
            &mut out,
            f,
            idx,
            None,
            &strings,
            errors,
            &canon,
            &module.funcs,
            &module.closures,
            &gidx,
            pfx,
            &HashMap::new(),
            &mut ext_decls,
        );
    }
    // 发射闭包函数
    for (idx, f) in module.closures.iter().enumerate() {
        let n_caps = closure_caps.get(&idx).copied().unwrap_or(0);
        emit_func(
            &mut out,
            f,
            idx,
            Some(n_caps),
            &strings,
            errors,
            &canon,
            &module.closures,
            &module.closures,
            &gidx,
            pfx,
            &HashMap::new(),
            &mut ext_decls,
        );
    }
    emit_ext_decls(&mut out, &ext_decls);
    emit_export_thunks(&mut out, module, pfx);
    if dll_mode {
        out
    } else {
        runtime_to_declares(&out)
    }
}

fn codegen_inner(
    module: &IrModule,
    errors: &ErrorCodeTable,
    links: &HashMap<String, String>,
    prefix: &str,
    helpers: bool,
) -> String {
    let strings = collect_strings(module);
    let canon: HashMap<String, Vec<usize>> = module
        .func_index
        .iter()
        .map(|(name, idxs)| (name.clone(), idxs.clone()))
        .collect();
    let gidx = globals_index(module);
    let closure_caps = closure_caps_map(module);
    let mut out = String::new();
    let mut ext_decls: Vec<(String, usize)> = Vec::new();
    emit_preamble(&mut out, &strings, &module.continuous, helpers);
    if helpers {
        emit_globals(&mut out, module);
    }
    for (idx, f) in module.funcs.iter().enumerate() {
        emit_func(
            &mut out,
            f,
            idx,
            None,
            &strings,
            errors,
            &canon,
            &module.funcs,
            &module.closures,
            &gidx,
            prefix,
            links,
            &mut ext_decls,
        );
    }
    // 发射闭包函数
    for (idx, f) in module.closures.iter().enumerate() {
        let n_caps = closure_caps.get(&idx).copied().unwrap_or(0);
        emit_func(
            &mut out,
            f,
            idx,
            Some(n_caps),
            &strings,
            errors,
            &canon,
            &module.closures,
            &module.closures,
            &gidx,
            prefix,
            links,
            &mut ext_decls,
        );
    }
    emit_ext_decls(&mut out, &ext_decls);
    emit_export_thunks(&mut out, module, prefix);
    if helpers {
        emit_main_wrapper(&mut out, module);
    }
    out
}

/// 生成「测试驱动」`.ll` 模块文本（导言 + 每个 `IrFunc` + `test fn` 跑器 main，Q-T5）。
/// 与 [`codegen`] 同导言与函数发射，仅入口包装从 `main` 换成 [`emit_test_runner`]。
pub fn codegen_tests(module: &IrModule, errors: &ErrorCodeTable) -> String {
    let strings = collect_strings(module);
    let canon: HashMap<String, Vec<usize>> = module
        .func_index
        .iter()
        .map(|(name, idxs)| (name.clone(), idxs.clone()))
        .collect();
    let gidx = globals_index(module);
    let closure_caps = closure_caps_map(module);
    let mut out = String::new();
    let mut ext_decls: Vec<(String, usize)> = Vec::new();
    emit_preamble(&mut out, &strings, &module.continuous, true);
    emit_globals(&mut out, module);
    for (idx, f) in module.funcs.iter().enumerate() {
        emit_func(
            &mut out,
            f,
            idx,
            None,
            &strings,
            errors,
            &canon,
            &module.funcs,
            &module.closures,
            &gidx,
            "",
            &HashMap::new(),
            &mut ext_decls,
        );
    }
    // 发射闭包函数
    for (idx, f) in module.closures.iter().enumerate() {
        let n_caps = closure_caps.get(&idx).copied().unwrap_or(0);
        emit_func(
            &mut out,
            f,
            idx,
            Some(n_caps),
            &strings,
            errors,
            &canon,
            &module.closures,
            &module.closures,
            &gidx,
            "",
            &HashMap::new(),
            &mut ext_decls,
        );
    }
    emit_ext_decls(&mut out, &ext_decls);
    emit_export_thunks(&mut out, module, "");
    emit_test_runner(&mut out, module, errors);
    out
}

/// 构建闭包索引 → 捕获数量映射（从 MakeClosure 指令的 captures.len() 收集）。
/// 扫描所有常规函数和闭包函数中的 MakeClosure 指令。
fn closure_caps_map(module: &IrModule) -> HashMap<usize, usize> {
    let mut m = HashMap::new();
    for f in module.funcs.iter().chain(module.closures.iter()) {
        for inst in &f.body {
            if let IrInst::MakeClosure { func, captures, .. } = inst {
                m.entry(*func).or_insert_with(|| captures.len());
            }
        }
    }
    m
}

/// 全局名 → `@.h_globals` 槽位（声明序，与 IR `IrModule::globals` 对齐）。
fn globals_index(module: &IrModule) -> HashMap<String, usize> {
    module
        .globals
        .iter()
        .enumerate()
        .map(|(i, g)| (g.clone(), i))
        .collect()
}

/// 模块级全局单元数组（`%Value` cell；LoadGlobal/StoreGlobal 寻址目标）。
fn emit_globals(out: &mut String, module: &IrModule) {
    if module.globals.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "@.h_globals = global [{} x %Value] zeroinitializer\n",
        module.globals.len()
    );
}

/// 收集全部字符串常量（去重、保序）。除 `Str` 常量外，还收集 Phase 2 指令携带的
/// 字面量字段名 / 类型名 / 枚举名变体名——它们需要以模块级全局字符串形式供 helper 取地址。
/// Phase 7：`io.print` 格式串的字面量段（`{}` 之间的字节）是格式串的子串，未必单独
/// 出现在 `Const Str` 中——必须在此登记，否则 `str_idx` 回退到 `.str.0` 打印错串。
/// 实例方法分派的拥有者名（`{Type}.{method}` 的 `{Type}`）同样登记。
fn collect_strings(module: &IrModule) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<String> = Vec::new();
    for f in module.funcs.iter().chain(module.closures.iter()) {
        let slot_consts = build_slot_consts(f);
        for inst in &f.body {
            match inst {
                IrInst::Const {
                    val: IrConst::Str(s),
                    ..
                } => push_str(s, &mut seen, &mut out),
                IrInst::Field { field, .. } | IrInst::StoreField { field, .. } => {
                    push_str(field, &mut seen, &mut out)
                }
                IrInst::MakeClass { ty, fields, .. } => {
                    push_str(ty, &mut seen, &mut out);
                    for (n, _) in fields {
                        push_str(n, &mut seen, &mut out);
                    }
                }
                IrInst::MakeEnum { name, variant, .. } => {
                    push_str(name, &mut seen, &mut out);
                    push_str(variant, &mut seen, &mut out);
                }
                IrInst::UnionSync { written, .. } => {
                    push_str(written, &mut seen, &mut out);
                    push_str("@union", &mut seen, &mut out);
                    push_str("@w", &mut seen, &mut out);
                }
                IrInst::MatchTest { pattern, .. } => {
                    // 模式描述符需字符串全局：Ident（bool/null/枚举变体）与 Str 模式。
                    // Error 模式在 codegen 期解析为错误码，无需字符串。
                    match pattern {
                        IrPattern::Ident(s) | IrPattern::Str(s) => push_str(s, &mut seen, &mut out),
                        _ => {}
                    }
                }
                // Phase 7：io.print 静态/实例调用的字面量段
                IrInst::Call { name, args, .. } => {
                    if is_io_print_name(name) {
                        collect_print_literals(args, &slot_consts, &mut seen, &mut out);
                    }
                }
                IrInst::CallMethod { method, args, .. } => {
                    if method == "print" {
                        collect_print_literals(args, &slot_consts, &mut seen, &mut out);
                        // 内建拥有者 "Io" 不在 func_index 中，须显式登记，
                        // 否则 `str_idx` 静默回退索引 0 导致 strcmp 链错配。
                        push_str("Io", &mut seen, &mut out);
                    }
                    // 用户方法拥有者名（`{Type}.{method}` 的 `{Type}`）
                    let suffix = format!(".{method}");
                    for key in module.func_index.keys() {
                        if let Some(owner) = key.strip_suffix(&suffix) {
                            push_str(owner, &mut seen, &mut out);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // [continuous] 类名（P11d）：`hc_deep_copy_cont` 门 strcmp 链目标
    for c in &module.continuous {
        push_str(c, &mut seen, &mut out);
    }
    out
}

/// 登记 `io.print(fmt, ...)` 格式串的全部字面量段（含空格式串的边界无段）。
fn collect_print_literals(
    args: &[usize],
    slot_consts: &HashMap<usize, IrConst>,
    seen: &mut HashMap<String, usize>,
    out: &mut Vec<String>,
) {
    let Some(fmt) = args
        .first()
        .and_then(|a| slot_consts.get(a))
        .and_then(|c| match c {
            IrConst::Str(s) => Some(s.clone()),
            _ => None,
        })
    else {
        return;
    };
    for seg in parse_print_fmt(&fmt, args) {
        if let PrintSeg::Lit(s) = seg {
            push_str(&s, seen, out);
        }
    }
}

/// 去重入表。
fn push_str(s: &str, seen: &mut HashMap<String, usize>, out: &mut Vec<String>) {
    if !seen.contains_key(s) {
        seen.insert(s.to_string(), out.len());
        out.push(s.to_string());
    }
}

/// 字符串全局下标与长度（GEP 指令发射用；LLVM 18+ 无常量表达式 GEP）。
fn str_idx(strings: &[String], s: &str) -> (usize, usize) {
    let idx = strings.iter().position(|x| x == s).unwrap_or(0);
    (idx, s.len() + 1)
}

/// 模板替换（避免 `format!` 对 LLVM `{`/`}` 的转义噪音）。
fn tpl(template: &str, subs: &[(&str, &str)]) -> String {
    let mut s = template.to_string();
    for (k, v) in subs {
        s = s.replace(k, v);
    }
    s
}

/// LLVM `c"..."` 字符串转义：可打印 ASCII 原样，其余 `\XX` 十六进制。
fn llvm_escape(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if (0x20..0x7e).contains(&b) && b != b'"' && b != b'\\' {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\{:02X}", b));
        }
    }
    s
}

struct Msg {
    key: &'static str,
    text: &'static str,
}

const MSGS: &[Msg] = &[
    Msg {
        key: "overflow",
        text: "error.Overflow: integer overflow",
    },
    Msg {
        key: "divzero",
        text: "error.DivisionByZero",
    },
    Msg {
        key: "assert",
        text: "error.AssertFailed",
    },
    Msg {
        key: "nofunc",
        text: "error.NoFunction",
    },
    Msg {
        key: "typeerr",
        text: "error.TypeError",
    },
    Msg {
        key: "badassign",
        text: "error.BadAssign",
    },
    Msg {
        key: "unhandled",
        text: "error: unhandled error value reached entry point",
    },
    // Phase 2 聚合运行时硬错误（对齐 tree-walking RtError 名称）
    Msg {
        key: "oom",
        text: "error.OutOfMemory",
    },
    // Phase 3 迭代/switch 硬错误
    Msg {
        key: "notiter",
        text: "error.NotIterable",
    },
    Msg {
        key: "indexoob",
        text: "error.IndexOutOfBounds",
    },
    Msg {
        key: "badindex",
        text: "error.BadIndex",
    },
    Msg {
        key: "notindexable",
        text: "error.NotIndexable",
    },
    Msg {
        key: "nullunwrap",
        text: "error.NullUnwrap",
    },
    Msg {
        key: "nofield",
        text: "error.NoField",
    },
    Msg {
        key: "tuplearity",
        text: "error.TupleArity",
    },
    // Phase 4 原生后端临时取舍：闭包/函数引用/间接调用/方法需原生 ABI 改造（Phase 8），
    // 当前响亮拒绝（error.NotCallable / error.NoMethod），禁止静默误编译
    // G4b（组 G 线程，定案 A）：spawn(f, …) 需把 callee 作为函数引用（FnRef）传递，
    // 原生 ABI 无函数值表示 → 同一 NotCallable 边界（子集外特性，非静默降级）。
    Msg {
        key: "notcallable",
        text: "error.NotCallable: function refs/closures/threads (spawn) not yet in native mode (Phase 8)",
    },
    Msg {
        key: "nomethod",
        text: "error.NoMethod: method calls not yet in native mode (Phase 7)",
    },
    // Phase 5 全局单元
    Msg {
        key: "noglobal",
        text: "error.NoGlobal: undefined global",
    },
    // Phase 7 内建：未实现内建响亮拒绝（禁止静默 Void 误编译）
    Msg {
        key: "builtin",
        text: "error.NotBuiltin: builtin not yet in native mode (Phase 7)",
    },
    Msg {
        key: "intcast",
        text: "error.IntCastOverflow: @intCast overflow",
    },
];
