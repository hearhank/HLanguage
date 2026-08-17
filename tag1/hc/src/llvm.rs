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

use crate::errorcodes::ErrorCodeTable;
use crate::ir::{IrBinOp, IrConst, IrFunc, IrInst, IrModule, IrPattern, IrUnOp};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

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
#[allow(dead_code)] // Phase 7 原生 ABI 落地时启用
const T_FN: i32 = 14;
#[allow(dead_code)] // Phase 7 原生 ABI 落地时启用
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
    let mut out = String::new();
    let mut ext_decls: Vec<(String, usize)> = Vec::new();
    emit_preamble(&mut out, &strings, &module.continuous, true);
    for (idx, f) in module.funcs.iter().enumerate() {
        emit_func(
            &mut out,
            f,
            idx,
            &strings,
            errors,
            &canon,
            &module.funcs,
            &gidx,
            &format!("{pkg}."),
            &HashMap::new(),
            &mut ext_decls,
        );
    }
    emit_ext_decls(&mut out, &ext_decls);
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
            &strings,
            errors,
            &canon,
            &module.funcs,
            &gidx,
            prefix,
            links,
            &mut ext_decls,
        );
    }
    emit_ext_decls(&mut out, &ext_decls);
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
    let mut out = String::new();
    let mut ext_decls: Vec<(String, usize)> = Vec::new();
    emit_preamble(&mut out, &strings, &module.continuous, true);
    emit_globals(&mut out, module);
    for (idx, f) in module.funcs.iter().enumerate() {
        emit_func(
            &mut out,
            f,
            idx,
            &strings,
            errors,
            &canon,
            &module.funcs,
            &gidx,
            "",
            &HashMap::new(),
            &mut ext_decls,
        );
    }
    emit_ext_decls(&mut out, &ext_decls);
    emit_test_runner(&mut out, module);
    out
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
    for f in &module.funcs {
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
    // Phase 4 原生后端临时取舍：闭包/函数引用/间接调用/方法需原生 ABI 改造（Phase 7），
    // 当前响亮拒绝（error.NotCallable / error.NoMethod），禁止静默误编译
    Msg {
        key: "notcallable",
        text: "error.NotCallable: closures/indirect calls not yet in native mode (Phase 7)",
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

// ---------- 导言 ----------

fn emit_preamble(
    out: &mut String,
    strings: &[String],
    continuous: &HashSet<String>,
    helpers: bool,
) {
    out.push_str("; H M3.3 LLVM 原生后端（自动生成；`zig cc file.ll -o file.exe`）\n\n");
    out.push_str("%Value = type { i32, i128 }\n");
    // Phase 2 聚合堆对象（聚合 `%Value` 的 data = 堆对象指针）
    out.push_str("%ArrObj = type { i64, %Value* }\n");
    out.push_str("%SliceObj = type { %Value*, i64, i64 }\n");
    out.push_str("%Field = type { i8*, %Value }\n");
    out.push_str("%ClassObj = type { i8*, i64, %Field* }\n");
    out.push_str("%EnumObj = type { i8*, i8*, %Value* }\n");
    out.push_str("%SeqInfo = type { %Value*, i64, i64 }\n");
    out.push_str("%FindRes = type { i1, %Value }\n");
    // Phase 3 迭代器（`for` 展开：源指针 + 写回目标）
    out.push_str("%IterItemObj = type { %Value*, i1 }\n");
    out.push_str("%IterObj = type { %IterItemObj*, i64, i64, i64 }\n\n");

    // 外部符号（libc + 溢出内建）
    out.push_str("declare i32 @strcmp(i8*, i8*)\n");
    out.push_str("declare i32 @memcmp(i8*, i8*, i64)\n");
    out.push_str("declare i32 @puts(i8*)\n");
    out.push_str("declare void @exit(i32) noreturn\n");
    out.push_str("declare i64 @strlen(i8*)\n");
    out.push_str("declare noalias i8* @malloc(i64)\n");
    out.push_str("declare noalias i8* @realloc(i8*, i64)\n");
    out.push_str("declare void @llvm.memcpy.p0i8.p0i8.i64(i8*, i8*, i64, i1)\n");
    out.push_str("declare { i128, i1 } @llvm.sadd.with.overflow.i128(i128, i128)\n");
    out.push_str("declare { i128, i1 } @llvm.ssub.with.overflow.i128(i128, i128)\n");
    out.push_str("declare { i128, i1 } @llvm.smul.with.overflow.i128(i128, i128)\n");
    // Phase 7 io.print / 数值显示辅助（libc 可变参 printf；fmod/fabs 判浮点整值；
    // sprintf 供 hc_fmt_float 数字→字符串）
    out.push_str("declare i32 @printf(i8*, ...)\n");
    out.push_str("declare i32 @sprintf(i8*, ...)\n");
    out.push_str("declare double @fmod(double, double)\n");
    out.push_str("declare double @fabs(double)\n");
    out.push_str("declare double @sqrt(double)\n");
    out.push_str("declare double @floor(double)\n");
    out.push_str("declare double @ceil(double)\n");
    out.push_str("declare double @round(double)\n\n");

    // 断言失败标志（全局；单线程顺序执行）
    out.push_str("@hc_fail_msg = global i8* null\n");
    out.push_str("@.void_value = private unnamed_addr constant %Value { i32 0, i128 0 }\n");
    out.push_str("@.empty_str_s = private unnamed_addr constant [1 x i8] c\"\\00\"\n");
    // `.len` 内建字段名字符串（`hc_field` 对 Str/Arr/Slice 判定用）
    out.push_str("@.hc_len = private unnamed_addr constant [4 x i8] c\"len\\00\"\n");
    // Phase 3 switch 模式 / 迭代器判型字符串
    out.push_str("@.hc_true = private unnamed_addr constant [5 x i8] c\"true\\00\"\n");
    out.push_str("@.hc_false = private unnamed_addr constant [6 x i8] c\"false\\00\"\n");
    out.push_str("@.hc_null = private unnamed_addr constant [5 x i8] c\"null\\00\"\n");
    out.push_str("@.hc_map = private unnamed_addr constant [4 x i8] c\"Map\\00\"\n");
    out.push_str("@.hc_kv = private unnamed_addr constant [3 x i8] c\"KV\\00\"\n");
    out.push_str("@.hc_key = private unnamed_addr constant [4 x i8] c\"key\\00\"\n");
    out.push_str("@.hc_value = private unnamed_addr constant [6 x i8] c\"value\\00\"\n");
    // Phase 7 io.print 显示格式常量（printf 格式串 + 定长字节写辅助）
    out.push_str("@.fmt_pct = private unnamed_addr constant [5 x i8] c\"%.*s\\00\"\n");
    out.push_str("@.fmt_s = private unnamed_addr constant [3 x i8] c\"%s\\00\"\n");
    out.push_str("@.fmt_one = private unnamed_addr constant [5 x i8] c\"%.1f\\00\"\n");
    out.push_str("@.fmt_g15 = private unnamed_addr constant [6 x i8] c\"%.15g\\00\"\n");
    out.push_str("@.fmt_e = private unnamed_addr constant [3 x i8] c\"%e\\00\"\n");
    out.push_str("@.hc_dash = private unnamed_addr constant [2 x i8] c\"-\\00\"\n");
    out.push_str("@.hc_lb = private unnamed_addr constant [2 x i8] c\"[\\00\"\n");
    out.push_str("@.hc_rb = private unnamed_addr constant [2 x i8] c\"]\\00\"\n");
    out.push_str("@.hc_comma = private unnamed_addr constant [3 x i8] c\", \\00\"\n");
    out.push_str("@.hc_bra_l = private unnamed_addr constant [4 x i8] c\" { \\00\"\n");
    out.push_str("@.hc_bra_r = private unnamed_addr constant [3 x i8] c\" }\\00\"\n");
    out.push_str("@.hc_eqs = private unnamed_addr constant [4 x i8] c\" = \\00\"\n");
    out.push_str("@.hc_errpre = private unnamed_addr constant [7 x i8] c\"error.\\00\"\n");
    out.push_str("@.hc_dot = private unnamed_addr constant [2 x i8] c\".\\00\"\n");
    out.push_str("@.hc_shallow = private unnamed_addr constant [8 x i8] c\"shallow\\00\"\n\n");

    // @typeOf 类型名常量（hc_typeof 返回值）
    out.push_str("@.t_i128 = private unnamed_addr constant [5 x i8] c\"i128\\00\"\n");
    out.push_str("@.t_f64 = private unnamed_addr constant [4 x i8] c\"f64\\00\"\n");
    out.push_str("@.t_bool = private unnamed_addr constant [5 x i8] c\"bool\\00\"\n");
    out.push_str("@.t_str = private unnamed_addr constant [6 x i8] c\"&[u8]\\00\"\n");
    out.push_str("@.t_arr = private unnamed_addr constant [6 x i8] c\"array\\00\"\n");
    out.push_str("@.t_slice = private unnamed_addr constant [6 x i8] c\"slice\\00\"\n");
    out.push_str("@.t_opt = private unnamed_addr constant [9 x i8] c\"optional\\00\"\n");
    out.push_str("@.t_err = private unnamed_addr constant [6 x i8] c\"error\\00\"\n");
    out.push_str("@.t_ptr = private unnamed_addr constant [8 x i8] c\"pointer\\00\"\n");
    out.push_str("@.t_fn = private unnamed_addr constant [3 x i8] c\"fn\\00\"\n");
    out.push_str("@.t_closure = private unnamed_addr constant [8 x i8] c\"closure\\00\"\n");
    out.push_str("@.t_end = private unnamed_addr constant [4 x i8] c\"end\\00\"\n");
    out.push_str("@.t_void = private unnamed_addr constant [5 x i8] c\"void\\00\"\n");
    out.push_str("@.t_iter = private unnamed_addr constant [7 x i8] c\"<iter>\\00\"\n\n");

    // main(io: Io) 单参入口：Io 值构造（fs/time/net 空子类，与 IR io_value_ir 对齐）
    out.push_str("@.t_io = private unnamed_addr constant [3 x i8] c\"Io\\00\"\n");
    out.push_str("@.t_fs = private unnamed_addr constant [3 x i8] c\"Fs\\00\"\n");
    out.push_str("@.t_time = private unnamed_addr constant [5 x i8] c\"Time\\00\"\n");
    out.push_str("@.t_net = private unnamed_addr constant [4 x i8] c\"Net\\00\"\n");
    out.push_str("@.f_fs = private unnamed_addr constant [3 x i8] c\"fs\\00\"\n");
    out.push_str("@.f_time = private unnamed_addr constant [5 x i8] c\"time\\00\"\n");
    out.push_str("@.f_net = private unnamed_addr constant [4 x i8] c\"net\\00\"\n\n");

    // 硬错误消息全局
    for m in MSGS {
        let n = m.text.len() + 1;
        let esc = llvm_escape(m.text.as_bytes());
        let _ = writeln!(
            out,
            "@.msg_{} = private unnamed_addr constant [{n} x i8] c\"{esc}\\00\"",
            m.key
        );
    }
    out.push('\n');

    // 字符串常量全局（去重后）
    for (i, s) in strings.iter().enumerate() {
        let n = s.len() + 1;
        let esc = llvm_escape(s.as_bytes());
        let _ = writeln!(
            out,
            "@.str.{i} = private unnamed_addr constant [{n} x i8] c\"{esc}\\00\""
        );
    }
    out.push('\n');

    // 中止 + 各硬错误无参包装
    out.push_str("define void @hc_abort(i8* %msg) {\n  call i32 @puts(i8* %msg)\n  call void @exit(i32 1)\n  unreachable\n}\n\n");
    for m in MSGS {
        let n = m.text.len() + 1;
        let _ = writeln!(out, "define void @hc_abort_{}() {{", m.key);
        let _ = writeln!(
            out,
            "  %p = getelementptr inbounds [{n} x i8], ptr @.msg_{}, i64 0, i64 0",
            m.key
        );
        let _ = writeln!(out, "  call void @hc_abort(i8* %p)");
        out.push_str("  unreachable\n}\n\n");
    }
    // 切片外函数调用（运行时 NoFunction 硬错误）
    out.push_str(
        "define %Value @hc_no_function() {\n  call void @hc_abort_nofunc()\n  unreachable\n}\n\n",
    );

    // 计算 helper（C3 库形态跳过——由链接的 exe 提供符号；中止基建保留）
    if helpers {
        emit_arith_helpers(out);
        emit_bit_helpers(out);
        emit_cmp_helpers(out);
        emit_unary_helpers(out);
        emit_assert_helpers(out);
        emit_pointer_helpers(out);
        emit_aggregate_helpers(out);
        emit_switch_helpers(out);
        emit_iter_helpers(out);
        emit_print_helpers(out);
        emit_scalar_builtin_helpers(out);
        emit_io_helper(out);
        emit_deep_copy_gate(out, strings, continuous);
    }
}

// ---------- 算术 helper（加/减/乘/除/模/欧几里得模） ----------

const TPL_OVERFLOW: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %chk_float
chk_float:
  %af = icmp eq i32 %ta, 3
  %bf = icmp eq i32 %tb, 3
  %any = or i1 %af, %bf
  br i1 %any, label %float_op, label %other
int_op:
  %res = call { i128, i1 } @INTRINSIC@(i128 %da, i128 %db)
  %rv = extractvalue { i128, i1 } %res, 0
  %ov = extractvalue { i128, i1 } %res, 1
  br i1 %ov, label %ovf, label %int_ok
int_ok:
  %i0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %i1 = insertvalue %Value %i0, i128 %rv, 1
  ret %Value %i1
ovf:
  call void @hc_abort_overflow()
  unreachable
float_op:
  %da_t = trunc i128 %da to i64
  %fa_raw = bitcast i64 %da_t to double
  %fa_int = sitofp i128 %da to double
  %fa = select i1 %af, double %fa_raw, double %fa_int
  %db_t = trunc i128 %db to i64
  %fb_raw = bitcast i64 %db_t to double
  %fb_int = sitofp i128 %db to double
  %fb = select i1 %bf, double %fb_raw, double %fb_int
  %fr = @FOP@ double %fa, %fb
  %fr_bits64 = bitcast double %fr to i64
  %fr_bits = zext i64 %fr_bits64 to i128
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i128 %fr_bits, 1
  ret %Value %f1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

const TPL_DIVMOD: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %chk_float
chk_float:
  %af = icmp eq i32 %ta, 3
  %bf = icmp eq i32 %tb, 3
  %any = or i1 %af, %bf
  br i1 %any, label %float_op, label %other
int_op:
  %bz = icmp eq i128 %db, 0
  br i1 %bz, label %divzero, label %int_ok
int_ok:
  %rv = @IOP@ i128 %da, %db
  %i0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %i1 = insertvalue %Value %i0, i128 %rv, 1
  ret %Value %i1
divzero:
  call void @hc_abort_divzero()
  unreachable
float_op:
  %da_t = trunc i128 %da to i64
  %fa_raw = bitcast i64 %da_t to double
  %fa_int = sitofp i128 %da to double
  %fa = select i1 %af, double %fa_raw, double %fa_int
  %db_t = trunc i128 %db to i64
  %fb_raw = bitcast i64 %db_t to double
  %fb_int = sitofp i128 %db to double
  %fb = select i1 %bf, double %fb_raw, double %fb_int
  %fr = @FOP@ double %fa, %fb
  %fr_bits64 = bitcast double %fr to i64
  %fr_bits = zext i64 %fr_bits64 to i128
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i128 %fr_bits, 1
  ret %Value %f1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

const TPL_EUCMOD: &str = r#"define %Value @hc_eucmod(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %chk_float
chk_float:
  %af = icmp eq i32 %ta, 3
  %bf = icmp eq i32 %tb, 3
  %any = or i1 %af, %bf
  br i1 %any, label %float_op, label %other
int_op:
  %bz = icmp eq i128 %db, 0
  br i1 %bz, label %divzero, label %int_ok
int_ok:
  %rm = srem i128 %da, %db
  %rneg = icmp slt i128 %rm, 0
  %dbneg = icmp slt i128 %db, 0
  %dbnegv = sub i128 0, %db
  %mabs = select i1 %dbneg, i128 %dbnegv, i128 %db
  %rm2 = add i128 %rm, %mabs
  %rv = select i1 %rneg, i128 %rm2, i128 %rm
  %i0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %i1 = insertvalue %Value %i0, i128 %rv, 1
  ret %Value %i1
divzero:
  call void @hc_abort_divzero()
  unreachable
float_op:
  %da_t = trunc i128 %da to i64
  %fa_raw = bitcast i64 %da_t to double
  %fa_int = sitofp i128 %da to double
  %fa = select i1 %af, double %fa_raw, double %fa_int
  %db_t = trunc i128 %db to i64
  %fb_raw = bitcast i64 %db_t to double
  %fb_int = sitofp i128 %db to double
  %fb = select i1 %bf, double %fb_raw, double %fb_int
  %fr = frem double %fa, %fb
  %fr_bits64 = bitcast double %fr to i64
  %fr_bits = zext i64 %fr_bits64 to i128
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i128 %fr_bits, 1
  ret %Value %f1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

fn emit_arith_helpers(out: &mut String) {
    for (fname, intr, fop) in [
        ("hc_add", "llvm.sadd.with.overflow.i128", "fadd"),
        ("hc_sub", "llvm.ssub.with.overflow.i128", "fsub"),
        ("hc_mul", "llvm.smul.with.overflow.i128", "fmul"),
    ] {
        let fname = format!("@{fname}");
        let intr = format!("@{intr}");
        out.push_str(&tpl(
            TPL_OVERFLOW,
            &[("@FNAME@", &fname), ("@INTRINSIC@", &intr), ("@FOP@", fop)],
        ));
        out.push('\n');
    }
    for (fname, iop, fop) in [("hc_div", "sdiv", "fdiv"), ("hc_mod", "srem", "frem")] {
        let fname = format!("@{fname}");
        out.push_str(&tpl(
            TPL_DIVMOD,
            &[("@FNAME@", &fname), ("@IOP@", iop), ("@FOP@", fop)],
        ));
        out.push('\n');
    }
    out.push_str(TPL_EUCMOD);
    out.push_str("\n\n");
}

// ---------- 位运算 helper（and/or/xor/shift） ----------

const TPL_BITOP: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %do, label %other
do:
  %r = @BOP@ i128 %da, %db
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %r, 1
  ret %Value %v1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

const TPL_SHIFT: &str = r#"define %Value @FNAME@(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %do, label %other
do:
  %sh = and i128 %db, 127
  %r = @SHOP@ i128 %da, %sh
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %r, 1
  ret %Value %v1
other:
  ret %Value { i32 2, i128 0 }
}
"#;

fn emit_bit_helpers(out: &mut String) {
    for (fname, bop) in [
        ("hc_bitand", "and"),
        ("hc_bitor", "or"),
        ("hc_bitxor", "xor"),
    ] {
        let fname = format!("@{fname}");
        out.push_str(&tpl(TPL_BITOP, &[("@FNAME@", &fname), ("@BOP@", bop)]));
        out.push('\n');
    }
    for (fname, shop) in [("hc_shl", "shl"), ("hc_shr", "ashr")] {
        let fname = format!("@{fname}");
        out.push_str(&tpl(TPL_SHIFT, &[("@FNAME@", &fname), ("@SHOP@", shop)]));
        out.push('\n');
    }
    out.push('\n');
}

// ---------- 比较 / 真值 helper ----------

/// 纯值相等（非指针操作数）；指针由 [`HC_EQ_DISPATCH`] 先归一化/分流后进入。
const HC_EQ_PLAIN: &str = r#"define i1 @hc_eq_plain(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %a_int = icmp eq i32 %ta, 2
  %b_int = icmp eq i32 %tb, 2
  %ii_case = and i1 %a_int, %b_int
  %ii_eq = icmp eq i128 %da, %db
  %ii_res = and i1 %ii_case, %ii_eq
  %b_float = icmp eq i32 %tb, 3
  %if_case = and i1 %a_int, %b_float
  %if_fa = sitofp i128 %da to double
  %db_t = trunc i128 %db to i64
  %if_fb = bitcast i64 %db_t to double
  %if_eq = fcmp oeq double %if_fa, %if_fb
  %if_res = and i1 %if_case, %if_eq
  %a_float = icmp eq i32 %ta, 3
  %fi_case = and i1 %a_float, %b_int
  %da_t = trunc i128 %da to i64
  %fi_fa = bitcast i64 %da_t to double
  %fi_fb = sitofp i128 %db to double
  %fi_eq = fcmp oeq double %fi_fa, %fi_fb
  %fi_res = and i1 %fi_case, %fi_eq
  %ff_case = and i1 %a_float, %b_float
  %ff_eq = fcmp oeq double %fi_fa, %if_fb
  %ff_res = and i1 %ff_case, %ff_eq
  %a_bool = icmp eq i32 %ta, 4
  %b_bool = icmp eq i32 %tb, 4
  %bb_case = and i1 %a_bool, %b_bool
  %bb_eq = icmp eq i128 %da, %db
  %bb_res = and i1 %bb_case, %bb_eq
  %a_str = icmp eq i32 %ta, 5
  %b_str = icmp eq i32 %tb, 5
  %ss_case = and i1 %a_str, %b_str
  %ss_pa = inttoptr i128 %da to i8*
  %ss_pb = inttoptr i128 %db to i8*
  %ss_es = getelementptr inbounds [1 x i8], ptr @.empty_str_s, i64 0, i64 0
  %ss_pa_s = select i1 %ss_case, i8* %ss_pa, i8* %ss_es
  %ss_pb_s = select i1 %ss_case, i8* %ss_pb, i8* %ss_es
  %ss_cmp = call i32 @strcmp(i8* %ss_pa_s, i8* %ss_pb_s)
  %ss_eq = icmp eq i32 %ss_cmp, 0
  %ss_res = and i1 %ss_case, %ss_eq
  %a_null = icmp eq i32 %ta, 1
  %b_null = icmp eq i32 %tb, 1
  %nn_res = and i1 %a_null, %b_null
  %a_void = icmp eq i32 %ta, 0
  %b_void = icmp eq i32 %tb, 0
  %vv_res = and i1 %a_void, %b_void
  %a_err = icmp eq i32 %ta, 6
  %b_err = icmp eq i32 %tb, 6
  %ee_case = and i1 %a_err, %b_err
  %ee_eq = icmp eq i128 %da, %db
  %ee_res = and i1 %ee_case, %ee_eq
  %r1 = or i1 %ii_res, %if_res
  %r2 = or i1 %r1, %fi_res
  %r3 = or i1 %r2, %ff_res
  %r4 = or i1 %r3, %bb_res
  %r5 = or i1 %r4, %ss_res
  %r6 = or i1 %r5, %nn_res
  %r7 = or i1 %r6, %vv_res
  %r8 = or i1 %r7, %ee_res
  ; Phase 2 聚合：两值均为聚合 tag（>=8）→ 深比较（递归 helper）
  %a_agg = icmp uge i32 %ta, 8
  %b_agg = icmp uge i32 %tb, 8
  %agg_case = and i1 %a_agg, %b_agg
  %agg_call = call i1 @hc_eq_agg(%Value %a, %Value %b)
  %agg_res = and i1 %agg_case, %agg_call
  %r9 = or i1 %r8, %agg_res
  ret i1 %r9
}
"#;

const HC_LT: &str = r#"define i1 @hc_lt(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %a_int = icmp eq i32 %ta, 2
  %b_int = icmp eq i32 %tb, 2
  %ii_case = and i1 %a_int, %b_int
  %ii_lt = icmp slt i128 %da, %db
  %ii_res = and i1 %ii_case, %ii_lt
  %b_float = icmp eq i32 %tb, 3
  %if_case = and i1 %a_int, %b_float
  %if_fa = sitofp i128 %da to double
  %db_t = trunc i128 %db to i64
  %if_fb = bitcast i64 %db_t to double
  %if_lt = fcmp olt double %if_fa, %if_fb
  %if_res = and i1 %if_case, %if_lt
  %a_float = icmp eq i32 %ta, 3
  %fi_case = and i1 %a_float, %b_int
  %da_t = trunc i128 %da to i64
  %fi_fa = bitcast i64 %da_t to double
  %fi_fb = sitofp i128 %db to double
  %fi_lt = fcmp olt double %fi_fa, %fi_fb
  %fi_res = and i1 %fi_case, %fi_lt
  %ff_case = and i1 %a_float, %b_float
  %ff_lt = fcmp olt double %fi_fa, %if_fb
  %ff_res = and i1 %ff_case, %ff_lt
  %a_bool = icmp eq i32 %ta, 4
  %b_bool = icmp eq i32 %tb, 4
  %bb_case = and i1 %a_bool, %b_bool
  %bb_lt = icmp slt i128 %da, %db
  %bb_res = and i1 %bb_case, %bb_lt
  %a_str = icmp eq i32 %ta, 5
  %b_str = icmp eq i32 %tb, 5
  %ss_case = and i1 %a_str, %b_str
  %ss_pa = inttoptr i128 %da to i8*
  %ss_pb = inttoptr i128 %db to i8*
  %ss_es = getelementptr inbounds [1 x i8], ptr @.empty_str_s, i64 0, i64 0
  %ss_pa_s = select i1 %ss_case, i8* %ss_pa, i8* %ss_es
  %ss_pb_s = select i1 %ss_case, i8* %ss_pb, i8* %ss_es
  %ss_cmp = call i32 @strcmp(i8* %ss_pa_s, i8* %ss_pb_s)
  %ss_lt = icmp slt i32 %ss_cmp, 0
  %ss_res = and i1 %ss_case, %ss_lt
  %a_ptr = icmp eq i32 %ta, 7
  %b_ptr = icmp eq i32 %tb, 7
  %pp_case = and i1 %a_ptr, %b_ptr
  %pp_lt = icmp slt i128 %da, %db
  %pp_res = and i1 %pp_case, %pp_lt
  %r1 = or i1 %ii_res, %if_res
  %r2 = or i1 %r1, %fi_res
  %r3 = or i1 %r2, %ff_res
  %r4 = or i1 %r3, %bb_res
  %r5 = or i1 %r4, %ss_res
  %r6 = or i1 %r5, %pp_res
  ret i1 %r6
}
"#;

const HC_TRUTHY: &str = r#"define i1 @hc_truthy(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_bool = icmp eq i32 %t, 4
  br i1 %is_bool, label %bool_, label %chk_int
bool_:
  %b = icmp ne i128 %d, 0
  ret i1 %b
chk_int:
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %int_, label %chk_float
int_:
  %i = icmp ne i128 %d, 0
  ret i1 %i
chk_float:
  %is_float = icmp eq i32 %t, 3
  br i1 %is_float, label %float_, label %chk_str
float_:
  %d_t = trunc i128 %d to i64
  %f = bitcast i64 %d_t to double
  %fn = fcmp une double %f, 0.000000e+00
  ret i1 %fn
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %str_, label %chk_null
str_:
  %p = inttoptr i128 %d to i8*
  %c = load i8, i8* %p
  %ne = icmp ne i8 %c, 0
  ret i1 %ne
chk_null:
  %is_null = icmp eq i32 %t, 1
  br i1 %is_null, label %null_, label %other
null_:
  ret i1 false
other:
  ret i1 true
}
"#;

const HC_IS_ERR: &str = r#"define i1 @hc_is_err(%Value %v) {
  %t = extractvalue %Value %v, 0
  %r = icmp eq i32 %t, 6
  ret i1 %r
}
"#;

const HC_IS_NULL: &str = r#"define i1 @hc_is_null(%Value %v) {
  %t = extractvalue %Value %v, 0
  %r = icmp eq i32 %t, 1
  ret i1 %r
}
"#;

const HC_BOOL: &str = r#"define %Value @hc_bool(i1 zeroext %b) {
  %d = zext i1 %b to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i128 %d, 1
  ret %Value %v1
}
"#;

/// 相等分派（Phase 1 指针）：两指针 → 载荷地址身份；否则解引用归一化后走纯值比较
/// （对齐 `IrValue::value_eq`：`(Ptr,Ptr)` 身份、`(Ptr,b)` 解引用）。
const HC_EQ_DISPATCH: &str = r#"define i1 @hc_eq(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %a_ptr = icmp eq i32 %ta, 7
  %b_ptr = icmp eq i32 %tb, 7
  %both_ptr = and i1 %a_ptr, %b_ptr
  br i1 %both_ptr, label %ptr_id, label %mixed
ptr_id:
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %id = icmp eq i128 %da, %db
  ret i1 %id
mixed:
  %an = call %Value @hc_deref(%Value %a)
  %bn = call %Value @hc_deref(%Value %b)
  %eq = call i1 @hc_eq_plain(%Value %an, %Value %bn)
  ret i1 %eq
}
"#;

fn emit_cmp_helpers(out: &mut String) {
    out.push_str(HC_EQ_PLAIN);
    out.push('\n');
    out.push_str(HC_EQ_DISPATCH);
    out.push('\n');
    out.push_str(HC_LT);
    out.push('\n');
    out.push_str(HC_TRUTHY);
    out.push('\n');
    out.push_str(HC_IS_ERR);
    out.push('\n');
    out.push_str(HC_IS_NULL);
    out.push('\n');
    out.push_str(HC_BOOL);
    out.push('\n');
}

// ---------- 一元运算 helper ----------

const HC_NEG: &str = r#"define %Value @hc_neg(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %int_, label %chk_float
int_:
  %n = sub i128 0, %d
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %n, 1
  ret %Value %v1
chk_float:
  %is_float = icmp eq i32 %t, 3
  br i1 %is_float, label %float_, label %err
float_:
  %d_t = trunc i128 %d to i64
  %f = bitcast i64 %d_t to double
  %nf = fneg double %f
  %bits64 = bitcast double %nf to i64
  %bits = zext i64 %bits64 to i128
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %f1 = insertvalue %Value %f0, i128 %bits, 1
  ret %Value %f1
err:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

const HC_BITNOT: &str = r#"define %Value @hc_bitnot(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %int_, label %err
int_:
  %n = xor i128 %d, -1
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %n, 1
  ret %Value %v1
err:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

const HC_NOT: &str = r#"define %Value @hc_not(%Value %v) {
  %b = call i1 @hc_truthy(%Value %v)
  %n = xor i1 %b, true
  %d = zext i1 %n to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i128 %d, 1
  ret %Value %v1
}
"#;

fn emit_unary_helpers(out: &mut String) {
    out.push_str(HC_NEG);
    out.push('\n');
    out.push_str(HC_BITNOT);
    out.push('\n');
    out.push_str(HC_NOT);
    out.push('\n');
}

// ---------- 指针 helper（Phase 1：取址 / 解引用 / 写穿） ----------

/// 解引用：tag 7 指针 → 载荷 `inttoptr` 后 load；非指针恒等（对齐
/// tree-walking `deref_value` 与 `IrValue::Deref` 的 identity 分支）。
const HC_DEREF: &str = r#"define %Value @hc_deref(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %is_ptr = icmp eq i32 %t, 7
  br i1 %is_ptr, label %deref, label %identity
deref:
  %d = extractvalue %Value %v, 1
  %pp = inttoptr i128 %d to %Value*
  %pv = load %Value, %Value* %pp
  ret %Value %pv
identity:
  ret %Value %v
}
"#;

/// 写穿：tag 7 指针 → `inttoptr` 后 store；非指针 → BadAssign（对齐
/// `StorePtr` 对非指针的硬错误）。
const HC_STORE_PTR: &str = r#"define void @hc_store_ptr(%Value %p, %Value %v) {
entry:
  %t = extractvalue %Value %p, 0
  %is_ptr = icmp eq i32 %t, 7
  br i1 %is_ptr, label %sp, label %err
sp:
  %d = extractvalue %Value %p, 1
  %pp = inttoptr i128 %d to %Value*
  store %Value %v, %Value* %pp
  ret void
err:
  call void @hc_abort_badassign()
  unreachable
}
"#;

/// 比较前归一化：指针解引用、普通值恒等。与 [`HC_EQ_DISPATCH`] 配合，
/// 让 `hc_eq` 在指针与非指针混合时对齐 `IrValue::value_eq`。
fn emit_pointer_helpers(out: &mut String) {
    out.push_str(HC_DEREF);
    out.push('\n');
    out.push_str(HC_STORE_PTR);
    out.push('\n');
}

// ---------- Phase 2 聚合 helper（堆对象分配 / 字段 / 索引 / 切片 / 字面量 / 深比较） ----------

const HC_ALLOC: &str = r#"define i8* @hc_alloc(i64 %size) {
entry:
  %z = icmp eq i64 %size, 0
  %sz = select i1 %z, i64 1, i64 %size
  %p = call i8* @malloc(i64 %sz)
  %ok = icmp ne i8* %p, null
  br i1 %ok, label %okb, label %oom
oom:
  call void @hc_abort_oom()
  unreachable
okb:
  ret i8* %p
}
"#;

const HC_MAKE_ARR: &str = r#"define %Value @hc_make_arr(i64 %n) {
entry:
  %arrsz = ptrtoint %ArrObj* getelementptr (%ArrObj, %ArrObj* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %arrsz)
  %op = bitcast i8* %raw to %ArrObj*
  %vsz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %isz = mul i64 %vsz, %n
  %iraw = call i8* @hc_alloc(i64 %isz)
  %items = bitcast i8* %iraw to %Value*
  %o0 = insertvalue %ArrObj undef, i64 %n, 0
  %o1 = insertvalue %ArrObj %o0, %Value* %items, 1
  store %ArrObj %o1, %ArrObj* %op
  %dp = ptrtoint %ArrObj* %op to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 8, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
}
"#;

const HC_ARR_SET: &str = r#"define void @hc_arr_set(%Value %arr, i64 %i, %Value %v) {
entry:
  %d = extractvalue %Value %arr, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %items = extractvalue %ArrObj %ao, 1
  %p = getelementptr %Value, %Value* %items, i64 %i
  store %Value %v, %Value* %p
  ret void
}
"#;

/// `append` 的原生实现：ArrObj（固定容量，无 cap 字段）原地扩容——分配新 items 缓冲、
/// 拷贝旧元素、`store` 回同一 ArrObj 指针。接收者槽/字段持有同一堆指针，写入即刻对所有
/// 别名可见（对齐 run_ir 的共享 cell 语义）。旧缓冲泄漏（本阶段整体无 free，一致取舍）。
const HC_APPEND: &str = r#"define void @hc_append(%Value %arr, %Value %v) {
entry:
  %d = extractvalue %Value %arr, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %oldlen = extractvalue %ArrObj %ao, 0
  %olditems = extractvalue %ArrObj %ao, 1
  %newlen = add i64 %oldlen, 1
  %vsz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %isz = mul i64 %vsz, %newlen
  %iraw = call i8* @hc_alloc(i64 %isz)
  %newitems = bitcast i8* %iraw to %Value*
  br label %ccond
ccond:
  %i = phi i64 [ 0, %entry ], [ %inext, %cbody ]
  %c = icmp ult i64 %i, %oldlen
  br i1 %c, label %cbody, label %cdone
cbody:
  %op1 = getelementptr %Value, %Value* %olditems, i64 %i
  %ov = load %Value, %Value* %op1
  %np1 = getelementptr %Value, %Value* %newitems, i64 %i
  store %Value %ov, %Value* %np1
  %inext = add i64 %i, 1
  br label %ccond
cdone:
  %last = getelementptr %Value, %Value* %newitems, i64 %oldlen
  store %Value %v, %Value* %last
  %a0 = insertvalue %ArrObj %ao, i64 %newlen, 0
  %a1 = insertvalue %ArrObj %a0, %Value* %newitems, 1
  store %ArrObj %a1, %ArrObj* %op
  ret void
}
"#;

/// `append_u64` 的原生实现：Int 值低 64 位按 LE 展开 8 字节，逐字节作为 Int 追加。
/// 复用 `hc_append`（逐次扩容，阶段内可接受；无自引用/别名迭代正确性要求）。
const HC_APPEND_U64: &str = r#"define void @hc_append_u64(%Value %arr, %Value %n) {
entry:
  %nd = extractvalue %Value %n, 1
  %nlow = trunc i128 %nd to i64
  %b0 = and i64 %nlow, 255
  %b0z = zext i64 %b0 to i128
  %v0a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v0b = insertvalue %Value %v0a, i128 %b0z, 1
  call void @hc_append(%Value %arr, %Value %v0b)
  %s1 = lshr i64 %nlow, 8
  %b1 = and i64 %s1, 255
  %b1z = zext i64 %b1 to i128
  %v1a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1b = insertvalue %Value %v1a, i128 %b1z, 1
  call void @hc_append(%Value %arr, %Value %v1b)
  %s2 = lshr i64 %nlow, 16
  %b2 = and i64 %s2, 255
  %b2z = zext i64 %b2 to i128
  %v2a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v2b = insertvalue %Value %v2a, i128 %b2z, 1
  call void @hc_append(%Value %arr, %Value %v2b)
  %s3 = lshr i64 %nlow, 24
  %b3 = and i64 %s3, 255
  %b3z = zext i64 %b3 to i128
  %v3a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v3b = insertvalue %Value %v3a, i128 %b3z, 1
  call void @hc_append(%Value %arr, %Value %v3b)
  %s4 = lshr i64 %nlow, 32
  %b4 = and i64 %s4, 255
  %b4z = zext i64 %b4 to i128
  %v4a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v4b = insertvalue %Value %v4a, i128 %b4z, 1
  call void @hc_append(%Value %arr, %Value %v4b)
  %s5 = lshr i64 %nlow, 40
  %b5 = and i64 %s5, 255
  %b5z = zext i64 %b5 to i128
  %v5a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v5b = insertvalue %Value %v5a, i128 %b5z, 1
  call void @hc_append(%Value %arr, %Value %v5b)
  %s6 = lshr i64 %nlow, 48
  %b6 = and i64 %s6, 255
  %b6z = zext i64 %b6 to i128
  %v6a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v6b = insertvalue %Value %v6a, i128 %b6z, 1
  call void @hc_append(%Value %arr, %Value %v6b)
  %s7 = lshr i64 %nlow, 56
  %b7 = and i64 %s7, 255
  %b7z = zext i64 %b7 to i128
  %v7a = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v7b = insertvalue %Value %v7a, i128 %b7z, 1
  call void @hc_append(%Value %arr, %Value %v7b)
  ret void
}
"#;

/// `extend` 的原生实现：`other` 为 Str（&[u8] 字节）或 Arr 时逐元素追加为 Int/元素值。
/// 对齐 run_ir `extend`（Str 逐字节 Int；Arr 元素克隆）。复用 `hc_append`。
const HC_EXTEND: &str = r#"define void @hc_extend(%Value %arr, %Value %other) {
entry:
  %ot = extractvalue %Value %other, 0
  %is_str = icmp eq i32 %ot, 5
  br i1 %is_str, label %estr, label %earr
estr:
  %od = extractvalue %Value %other, 1
  %osp = inttoptr i128 %od to i8*
  %cnt = call i64 @strlen(i8* %osp)
  br label %scond
scond:
  %si = phi i64 [ 0, %estr ], [ %snext, %sbody ]
  %sd = icmp ult i64 %si, %cnt
  br i1 %sd, label %sbody, label %sdone
sbody:
  %sp = getelementptr i8, i8* %osp, i64 %si
  %sb = load i8, i8* %sp
  %sz = zext i8 %sb to i128
  %sv0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %sv1 = insertvalue %Value %sv0, i128 %sz, 1
  call void @hc_append(%Value %arr, %Value %sv1)
  %snext = add i64 %si, 1
  br label %scond
sdone:
  ret void
earr:
  %od2 = extractvalue %Value %other, 1
  %op2 = inttoptr i128 %od2 to %ArrObj*
  %ao2 = load %ArrObj, %ArrObj* %op2
  %elen = extractvalue %ArrObj %ao2, 0
  %eitems = extractvalue %ArrObj %ao2, 1
  br label %econd
econd:
  %ei = phi i64 [ 0, %earr ], [ %enext, %ebody ]
  %ed = icmp ult i64 %ei, %elen
  br i1 %ed, label %ebody, label %edone
ebody:
  %ep = getelementptr %Value, %Value* %eitems, i64 %ei
  %ev = load %Value, %Value* %ep
  call void @hc_append(%Value %arr, %Value %ev)
  %enext = add i64 %ei, 1
  br label %econd
edone:
  ret void
}
"#;

const HC_MAKE_CLASS: &str = r#"define %Value @hc_make_class(i8* %ty, i64 %n) {
entry:
  %os = ptrtoint %ClassObj* getelementptr (%ClassObj, %ClassObj* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %os)
  %op = bitcast i8* %raw to %ClassObj*
  %fsz = ptrtoint %Field* getelementptr (%Field, %Field* null, i32 1) to i64
  %fs = mul i64 %fsz, %n
  %fraw = call i8* @hc_alloc(i64 %fs)
  %fields = bitcast i8* %fraw to %Field*
  %o0 = insertvalue %ClassObj undef, i8* %ty, 0
  %o1 = insertvalue %ClassObj %o0, i64 %n, 1
  %o2 = insertvalue %ClassObj %o1, %Field* %fields, 2
  store %ClassObj %o2, %ClassObj* %op
  %dp = ptrtoint %ClassObj* %op to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 10, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
}
"#;

const HC_CLASS_SET: &str = r#"define void @hc_class_set(%Value %obj, i64 %i, i8* %fname, %Value %v) {
entry:
  %d = extractvalue %Value %obj, 1
  %op = inttoptr i128 %d to %ClassObj*
  %ao = load %ClassObj, %ClassObj* %op
  %fields = extractvalue %ClassObj %ao, 2
  %p = getelementptr %Field, %Field* %fields, i64 %i
  %f0 = insertvalue %Field undef, i8* %fname, 0
  %f1 = insertvalue %Field %f0, %Value %v, 1
  store %Field %f1, %Field* %p
  ret void
}
"#;

const HC_MAKE_ENUM: &str = r#"define %Value @hc_make_enum(i8* %name, i8* %variant, %Value* %payload) {
entry:
  %os = ptrtoint %EnumObj* getelementptr (%EnumObj, %EnumObj* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %os)
  %op = bitcast i8* %raw to %EnumObj*
  %o0 = insertvalue %EnumObj undef, i8* %name, 0
  %o1 = insertvalue %EnumObj %o0, i8* %variant, 1
  %o2 = insertvalue %EnumObj %o1, %Value* %payload, 2
  store %EnumObj %o2, %EnumObj* %op
  %dp = ptrtoint %EnumObj* %op to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 11, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
}
"#;

const HC_UNWRAP: &str = r#"define %Value @hc_unwrap(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %is_opt = icmp eq i32 %t, 1
  br i1 %is_opt, label %opt, label %id
id:
  ret %Value %v
opt:
  %d = extractvalue %Value %v, 1
  %is_none = icmp eq i128 %d, 0
  br i1 %is_none, label %nullb, label %some
nullb:
  call void @hc_abort_nullunwrap()
  unreachable
some:
  %p = inttoptr i128 %d to %Value*
  %pv = load %Value, %Value* %p
  ret %Value %pv
}
"#;

const HC_INDEX: &str = r#"define %Value @hc_index(%Value %base, %Value %idx) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %it = extractvalue %Value %idx, 0
  %is_int = icmp eq i32 %it, 2
  br i1 %is_int, label %have, label %badidx
badidx:
  call void @hc_abort_badindex()
  unreachable
have:
  %id = extractvalue %Value %idx, 1
  %ix = trunc i128 %id to i64
  %neg = icmp slt i64 %ix, 0
  br i1 %neg, label %badidx, label %bt
bt:
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %chk_slc
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %len = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %oob = icmp uge i64 %ix, %len
  br i1 %oob, label %oob_err, label %ok_arr
ok_arr:
  %p = getelementptr %Value, %Value* %items, i64 %ix
  %v = load %Value, %Value* %p
  ret %Value %v
oob_err:
  call void @hc_abort_indexoob()
  unreachable
chk_slc:
  %is_slc = icmp eq i32 %t, 9
  br i1 %is_slc, label %slc, label %chk_str
slc:
  %d2 = extractvalue %Value %b, 1
  %op2 = inttoptr i128 %d2 to %SliceObj*
  %so = load %SliceObj, %SliceObj* %op2
  %sitems = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %oob2 = icmp uge i64 %ix, %slen
  br i1 %oob2, label %oob_err, label %ok_slc
ok_slc:
  %ii = add i64 %sstart, %ix
  %p2 = getelementptr %Value, %Value* %sitems, i64 %ii
  %v2 = load %Value, %Value* %p2
  ret %Value %v2
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %strb, label %notidx
strb:
  %d3 = extractvalue %Value %b, 1
  %sp = inttoptr i128 %d3 to i8*
  %blen = call i64 @strlen(i8* %sp)
  %oob3 = icmp uge i64 %ix, %blen
  br i1 %oob3, label %oob_err, label %ok_str
ok_str:
  %sp2 = getelementptr i8, i8* %sp, i64 %ix
  %bval = load i8, i8* %sp2
  %bint = zext i8 %bval to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %bint, 1
  ret %Value %v1
notidx:
  call void @hc_abort_notindexable()
  unreachable
}
"#;

const HC_STORE_INDEX: &str = r#"define void @hc_store_index(%Value %base, %Value %idx, %Value %v) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %it = extractvalue %Value %idx, 0
  %is_int = icmp eq i32 %it, 2
  br i1 %is_int, label %have, label %badidx
badidx:
  call void @hc_abort_badindex()
  unreachable
have:
  %id = extractvalue %Value %idx, 1
  %ix = trunc i128 %id to i64
  %neg = icmp slt i64 %ix, 0
  br i1 %neg, label %badidx, label %bt
bt:
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %err
err:
  call void @hc_abort_typeerr()
  unreachable
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %len = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %oob = icmp uge i64 %ix, %len
  br i1 %oob, label %oob_err, label %ok
ok:
  %p = getelementptr %Value, %Value* %items, i64 %ix
  store %Value %v, %Value* %p
  ret void
oob_err:
  call void @hc_abort_indexoob()
  unreachable
}
"#;

const HC_SLICE: &str = r#"define %Value @hc_slice(%Value %base, %Value %lo, %Value %hi) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %lt = extractvalue %Value %lo, 0
  %li = icmp eq i32 %lt, 2
  br i1 %li, label %have_lo, label %badidx
badidx:
  call void @hc_abort_badindex()
  unreachable
have_lo:
  %ld = extractvalue %Value %lo, 1
  %lox = trunc i128 %ld to i64
  %lneg = icmp slt i64 %lox, 0
  br i1 %lneg, label %badidx, label %have_hi
have_hi:
  %het = extractvalue %Value %hi, 0
  %is_end = icmp eq i32 %het, 12
  br i1 %is_end, label %set_open, label %chk_hint
chk_hint:
  %hi_int = icmp eq i32 %het, 2
  br i1 %hi_int, label %set_closed, label %badidx
set_open:
  br label %bt
set_closed:
  %hdx = extractvalue %Value %hi, 1
  %hixc = trunc i128 %hdx to i64
  %hneg = icmp slt i64 %hixc, 0
  br i1 %hneg, label %badidx, label %bt
bt:
  %open = phi i1 [ true, %set_open ], [ false, %set_closed ]
  %hix = phi i64 [ 0, %set_open ], [ %hixc, %set_closed ]
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %chk_slc
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %len = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %hsel = select i1 %open, i64 %len, i64 %hix
  %hbad = icmp ugt i64 %hsel, %len
  %lbad = icmp ugt i64 %lox, %len
  %bad = or i1 %hbad, %lbad
  br i1 %bad, label %oob_err, label %mk_slc
mk_slc:
  %nl = sub i64 %hsel, %lox
  %os2 = ptrtoint %SliceObj* getelementptr (%SliceObj, %SliceObj* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %os2)
  %op2 = bitcast i8* %raw to %SliceObj*
  %s0 = insertvalue %SliceObj undef, %Value* %items, 0
  %s1 = insertvalue %SliceObj %s0, i64 %lox, 1
  %s2 = insertvalue %SliceObj %s1, i64 %nl, 2
  store %SliceObj %s2, %SliceObj* %op2
  %dp = ptrtoint %SliceObj* %op2 to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 9, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
oob_err:
  call void @hc_abort_indexoob()
  unreachable
chk_slc:
  %is_slc = icmp eq i32 %t, 9
  br i1 %is_slc, label %slc, label %chk_str
slc:
  %d2 = extractvalue %Value %b, 1
  %op3 = inttoptr i128 %d2 to %SliceObj*
  %so = load %SliceObj, %SliceObj* %op3
  %sitems = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %h2 = select i1 %open, i64 %slen, i64 %hix
  %hbad2 = icmp ugt i64 %h2, %slen
  %lbad2 = icmp ugt i64 %lox, %slen
  %bad2 = or i1 %hbad2, %lbad2
  br i1 %bad2, label %oob_err, label %mk_slc2
mk_slc2:
  %nl2 = sub i64 %h2, %lox
  %nstart = add i64 %sstart, %lox
  %os3 = ptrtoint %SliceObj* getelementptr (%SliceObj, %SliceObj* null, i32 1) to i64
  %raw2 = call i8* @hc_alloc(i64 %os3)
  %op4 = bitcast i8* %raw2 to %SliceObj*
  %t0 = insertvalue %SliceObj undef, %Value* %sitems, 0
  %t1 = insertvalue %SliceObj %t0, i64 %nstart, 1
  %t2 = insertvalue %SliceObj %t1, i64 %nl2, 2
  store %SliceObj %t2, %SliceObj* %op4
  %dp2 = ptrtoint %SliceObj* %op4 to i128
  %w0 = insertvalue %Value { i32 0, i128 0 }, i32 9, 0
  %w1 = insertvalue %Value %w0, i128 %dp2, 1
  ret %Value %w1
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %strb, label %notidx
strb:
  %d3 = extractvalue %Value %b, 1
  %sp = inttoptr i128 %d3 to i8*
  %blen = call i64 @strlen(i8* %sp)
  %h3 = select i1 %open, i64 %blen, i64 %hix
  %hbad3 = icmp ugt i64 %h3, %blen
  %lbad3 = icmp ugt i64 %lox, %blen
  %bad3 = or i1 %hbad3, %lbad3
  br i1 %bad3, label %oob_err, label %mk_str
mk_str:
  %nl3 = sub i64 %h3, %lox
  %nbytes = add i64 %nl3, 1
  %buf = call i8* @hc_alloc(i64 %nbytes)
  %srcp = getelementptr i8, i8* %sp, i64 %lox
  call void @llvm.memcpy.p0i8.p0i8.i64(i8* %buf, i8* %srcp, i64 %nl3, i1 false)
  %nulp = getelementptr i8, i8* %buf, i64 %nl3
  store i8 0, i8* %nulp
  %pi = ptrtoint i8* %buf to i128
  %x0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %x1 = insertvalue %Value %x0, i128 %pi, 1
  ret %Value %x1
notidx:
  call void @hc_abort_notindexable()
  unreachable
}
"#;

const HC_STORE_SLICE: &str = r#"define void @hc_store_slice(%Value %base, %Value %lo, %Value %hi, %Value %v) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %err
err:
  call void @hc_abort_typeerr()
  unreachable
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %len = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %lt = extractvalue %Value %lo, 0
  %li = icmp eq i32 %lt, 2
  br i1 %li, label %hl, label %badidx
badidx:
  call void @hc_abort_badindex()
  unreachable
hl:
  %ld = extractvalue %Value %lo, 1
  %lox = trunc i128 %ld to i64
  %lneg = icmp slt i64 %lox, 0
  br i1 %lneg, label %badidx, label %hh
hh:
  %het = extractvalue %Value %hi, 0
  %is_end = icmp eq i32 %het, 12
  br i1 %is_end, label %badidx, label %chk_hint
chk_hint:
  %hi_int = icmp eq i32 %het, 2
  br i1 %hi_int, label %hh2, label %badidx
hh2:
  %hd = extractvalue %Value %hi, 1
  %hix = trunc i128 %hd to i64
  %hneg = icmp slt i64 %hix, 0
  br i1 %hneg, label %badidx, label %bounds
bounds:
  %hbad = icmp ugt i64 %hix, %len
  %lbad = icmp ugt i64 %lox, %len
  %bad = or i1 %hbad, %lbad
  br i1 %bad, label %oob, label %src
oob:
  call void @hc_abort_indexoob()
  unreachable
src:
  %sv = call %Value @hc_deref(%Value %v)
  %st = extractvalue %Value %sv, 0
  %is_src = icmp eq i32 %st, 8
  br i1 %is_src, label %snap, label %done
done:
  ret void
snap:
  %sd = extractvalue %Value %sv, 1
  %sop = inttoptr i128 %sd to %ArrObj*
  %sao = load %ArrObj, %ArrObj* %sop
  %slen = extractvalue %ArrObj %sao, 0
  %sitems = extractvalue %ArrObj %sao, 1
  %vsz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %bsz = mul i64 %vsz, %slen
  %braw = call i8* @hc_alloc(i64 %bsz)
  %buf = bitcast i8* %braw to %Value*
  %bytes = mul i64 %slen, %vsz
  %sp_ = bitcast %Value* %sitems to i8*
  %bp_ = bitcast %Value* %buf to i8*
  call void @llvm.memcpy.p0i8.p0i8.i64(i8* %bp_, i8* %sp_, i64 %bytes, i1 false)
  br label %loop
loop:
  %k = phi i64 [ 0, %snap ], [ %k2, %next ]
  %kdone = icmp uge i64 %k, %slen
  br i1 %kdone, label %done, label %body
body:
  %dst_i = add i64 %lox, %k
  %inb = icmp ult i64 %dst_i, %len
  br i1 %inb, label %write, label %next
write:
  %bp = getelementptr %Value, %Value* %buf, i64 %k
  %bv = load %Value, %Value* %bp
  %dp_ = getelementptr %Value, %Value* %items, i64 %dst_i
  store %Value %bv, %Value* %dp_
  br label %next
next:
  %k2 = add i64 %k, 1
  br label %loop
}
"#;

const HC_CLASS_FIND: &str = r#"define %FindRes @hc_class_find(%Value %base, i8* %field) {
entry:
  %t = extractvalue %Value %base, 0
  %is = icmp eq i32 %t, 10
  br i1 %is, label %cls, label %nf
cls:
  %d = extractvalue %Value %base, 1
  %op = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %op
  %cnt = extractvalue %ClassObj %co, 1
  %fs = extractvalue %ClassObj %co, 2
  br label %loop
loop:
  %i = phi i64 [ 0, %cls ], [ %i2, %next ]
  %done = icmp uge i64 %i, %cnt
  br i1 %done, label %nf, label %body
body:
  %fp = getelementptr %Field, %Field* %fs, i64 %i
  %f = load %Field, %Field* %fp
  %fn = extractvalue %Field %f, 0
  %c = call i32 @strcmp(i8* %fn, i8* %field)
  %eq = icmp eq i32 %c, 0
  br i1 %eq, label %found, label %next
next:
  %i2 = add i64 %i, 1
  br label %loop
found:
  %fv = extractvalue %Field %f, 1
  %r0 = insertvalue %FindRes undef, i1 true, 0
  %r1 = insertvalue %FindRes %r0, %Value %fv, 1
  ret %FindRes %r1
nf:
  ret %FindRes { i1 false, %Value undef }
}
"#;

const HC_FIELD: &str = r#"define %Value @hc_field(%Value %base, i8* %field) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %t = extractvalue %Value %b, 0
  %is_class = icmp eq i32 %t, 10
  br i1 %is_class, label %cls, label %chk_str
cls:
  %fr = call %FindRes @hc_class_find(%Value %b, i8* %field)
  %ok = extractvalue %FindRes %fr, 0
  br i1 %ok, label %found, label %nf
found:
  %v = extractvalue %FindRes %fr, 1
  ret %Value %v
nf:
  call void @hc_abort_nofield()
  unreachable
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %strb, label %chk_arr
strb:
  %lenp = getelementptr inbounds [4 x i8], ptr @.hc_len, i64 0, i64 0
  %c = call i32 @strcmp(i8* %field, i8* %lenp)
  %iseq = icmp eq i32 %c, 0
  br i1 %iseq, label %len_ok, label %nf
len_ok:
  %d = extractvalue %Value %b, 1
  %sp = inttoptr i128 %d to i8*
  %bl = call i64 @strlen(i8* %sp)
  %bi = zext i64 %bl to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %bi, 1
  ret %Value %v1
chk_arr:
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arrb, label %chk_slc
arrb:
  %lenp2 = getelementptr inbounds [4 x i8], ptr @.hc_len, i64 0, i64 0
  %c2 = call i32 @strcmp(i8* %field, i8* %lenp2)
  %iseq2 = icmp eq i32 %c2, 0
  br i1 %iseq2, label %arr_len, label %nf
arr_len:
  %d2 = extractvalue %Value %b, 1
  %op = inttoptr i128 %d2 to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %al = extractvalue %ArrObj %ao, 0
  %ai = zext i64 %al to i128
  %w0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %w1 = insertvalue %Value %w0, i128 %ai, 1
  ret %Value %w1
chk_slc:
  %is_slc = icmp eq i32 %t, 9
  br i1 %is_slc, label %slcb, label %nf
slcb:
  %lenp3 = getelementptr inbounds [4 x i8], ptr @.hc_len, i64 0, i64 0
  %c3 = call i32 @strcmp(i8* %field, i8* %lenp3)
  %iseq3 = icmp eq i32 %c3, 0
  br i1 %iseq3, label %slc_len, label %nf
slc_len:
  %d3 = extractvalue %Value %b, 1
  %op2 = inttoptr i128 %d3 to %SliceObj*
  %so = load %SliceObj, %SliceObj* %op2
  %sl = extractvalue %SliceObj %so, 2
  %si = zext i64 %sl to i128
  %u0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %u1 = insertvalue %Value %u0, i128 %si, 1
  ret %Value %u1
}
"#;

const HC_STORE_FIELD: &str = r#"define void @hc_store_field(%Value %base, i8* %field, %Value %v) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %t = extractvalue %Value %b, 0
  %is = icmp eq i32 %t, 10
  br i1 %is, label %cls, label %err
err:
  call void @hc_abort_typeerr()
  unreachable
cls:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %op
  %cnt = extractvalue %ClassObj %co, 1
  %fs = extractvalue %ClassObj %co, 2
  br label %loop
loop:
  %i = phi i64 [ 0, %cls ], [ %i2, %next ]
  %done = icmp uge i64 %i, %cnt
  br i1 %done, label %append, label %body
body:
  %fp = getelementptr %Field, %Field* %fs, i64 %i
  %f = load %Field, %Field* %fp
  %fn = extractvalue %Field %f, 0
  %c = call i32 @strcmp(i8* %fn, i8* %field)
  %eq = icmp eq i32 %c, 0
  br i1 %eq, label %found, label %next
next:
  %i2 = add i64 %i, 1
  br label %loop
found:
  %nf0 = insertvalue %Field undef, i8* %field, 0
  %nf1 = insertvalue %Field %nf0, %Value %v, 1
  store %Field %nf1, %Field* %fp
  ret void
append:
  %newcnt = add i64 %cnt, 1
  %fsz = ptrtoint %Field* getelementptr (%Field, %Field* null, i32 1) to i64
  %newsz = mul i64 %fsz, %newcnt
  %fsp = bitcast %Field* %fs to i8*
  %newp = call i8* @realloc(i8* %fsp, i64 %newsz)
  %newf = bitcast i8* %newp to %Field*
  %fp2 = getelementptr %Field, %Field* %newf, i64 %cnt
  %n2 = insertvalue %Field undef, i8* %field, 0
  %n3 = insertvalue %Field %n2, %Value %v, 1
  store %Field %n3, %Field* %fp2
  %c0 = insertvalue %ClassObj %co, i64 %newcnt, 1
  %c1 = insertvalue %ClassObj %c0, %Field* %newf, 2
  store %ClassObj %c1, %ClassObj* %op
  ret void
}
"#;

const HC_SEQ_INFO: &str = r#"define %SeqInfo @hc_seq_info(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %slc
arr:
  %ap = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %ap
  %alen = extractvalue %ArrObj %ao, 0
  %aitems = extractvalue %ArrObj %ao, 1
  %r0 = insertvalue %SeqInfo undef, %Value* %aitems, 0
  %r1 = insertvalue %SeqInfo %r0, i64 0, 1
  %r2 = insertvalue %SeqInfo %r1, i64 %alen, 2
  ret %SeqInfo %r2
slc:
  %sp = inttoptr i128 %d to %SliceObj*
  %so = load %SliceObj, %SliceObj* %sp
  %sitems = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %q0 = insertvalue %SeqInfo undef, %Value* %sitems, 0
  %q1 = insertvalue %SeqInfo %q0, i64 %sstart, 1
  %q2 = insertvalue %SeqInfo %q1, i64 %slen, 2
  ret %SeqInfo %q2
}
"#;

const HC_EQ_AGG: &str = r#"define i1 @hc_eq_agg(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %a_end = icmp eq i32 %ta, 12
  %b_end = icmp eq i32 %tb, 12
  %ee = and i1 %a_end, %b_end
  br i1 %ee, label %ret_true, label %chk_seq
chk_seq:
  %a_arr = icmp eq i32 %ta, 8
  %a_slc = icmp eq i32 %ta, 9
  %a_seq = or i1 %a_arr, %a_slc
  %b_arr = icmp eq i32 %tb, 8
  %b_slc = icmp eq i32 %tb, 9
  %b_seq = or i1 %b_arr, %b_slc
  %both_seq = and i1 %a_seq, %b_seq
  br i1 %both_seq, label %seq, label %chk_cls
seq:
  %ai = call %SeqInfo @hc_seq_info(%Value %a)
  %bi = call %SeqInfo @hc_seq_info(%Value %b)
  %abase = extractvalue %SeqInfo %ai, 0
  %astart = extractvalue %SeqInfo %ai, 1
  %alen = extractvalue %SeqInfo %ai, 2
  %bbase = extractvalue %SeqInfo %bi, 0
  %bstart = extractvalue %SeqInfo %bi, 1
  %blen = extractvalue %SeqInfo %bi, 2
  %le = icmp eq i64 %alen, %blen
  br i1 %le, label %loop, label %ret_false
loop:
  %k = phi i64 [ 0, %seq ], [ %k2, %lnext ]
  %kdone = icmp uge i64 %k, %alen
  br i1 %kdone, label %ret_true, label %lbody
lbody:
  %ai2 = add i64 %astart, %k
  %bi2 = add i64 %bstart, %k
  %ap = getelementptr %Value, %Value* %abase, i64 %ai2
  %bp = getelementptr %Value, %Value* %bbase, i64 %bi2
  %av = load %Value, %Value* %ap
  %bv = load %Value, %Value* %bp
  %eq = call i1 @hc_eq(%Value %av, %Value %bv)
  br i1 %eq, label %lnext, label %ret_false
lnext:
  %k2 = add i64 %k, 1
  br label %loop
chk_cls:
  %a_cls = icmp eq i32 %ta, 10
  %b_cls = icmp eq i32 %tb, 10
  %both_cls = and i1 %a_cls, %b_cls
  br i1 %both_cls, label %cls, label %chk_enum
cls:
  %ad = extractvalue %Value %a, 1
  %bd = extractvalue %Value %b, 1
  %aop = inttoptr i128 %ad to %ClassObj*
  %bop = inttoptr i128 %bd to %ClassObj*
  %ao = load %ClassObj, %ClassObj* %aop
  %bo = load %ClassObj, %ClassObj* %bop
  %an = extractvalue %ClassObj %ao, 0
  %bn = extractvalue %ClassObj %bo, 0
  %acnt = extractvalue %ClassObj %ao, 1
  %bcnt = extractvalue %ClassObj %bo, 1
  %afs = extractvalue %ClassObj %ao, 2
  %ncmp = call i32 @strcmp(i8* %an, i8* %bn)
  %neq = icmp eq i32 %ncmp, 0
  %ceq = icmp eq i64 %acnt, %bcnt
  %pre = and i1 %neq, %ceq
  br i1 %pre, label %cloop, label %ret_false
cloop:
  %i = phi i64 [ 0, %cls ], [ %i2, %cnext ]
  %idone = icmp uge i64 %i, %acnt
  br i1 %idone, label %ret_true, label %cbody
cbody:
  %fp = getelementptr %Field, %Field* %afs, i64 %i
  %f = load %Field, %Field* %fp
  %fname = extractvalue %Field %f, 0
  %fval = extractvalue %Field %f, 1
  %bres = call %FindRes @hc_class_find(%Value %b, i8* %fname)
  %bfound = extractvalue %FindRes %bres, 0
  br i1 %bfound, label %cmatch, label %ret_false
cmatch:
  %bval = extractvalue %FindRes %bres, 1
  %fveq = call i1 @hc_eq(%Value %fval, %Value %bval)
  br i1 %fveq, label %cnext, label %ret_false
cnext:
  %i2 = add i64 %i, 1
  br label %cloop
chk_enum:
  %a_enum = icmp eq i32 %ta, 11
  %b_enum = icmp eq i32 %tb, 11
  %both_enum = and i1 %a_enum, %b_enum
  br i1 %both_enum, label %enm, label %ret_false
enm:
  %ed = extractvalue %Value %a, 1
  %fd = extractvalue %Value %b, 1
  %eop = inttoptr i128 %ed to %EnumObj*
  %fop = inttoptr i128 %fd to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %eop
  %fo = load %EnumObj, %EnumObj* %fop
  %en = extractvalue %EnumObj %eo, 0
  %fn_ = extractvalue %EnumObj %fo, 0
  %ev = extractvalue %EnumObj %eo, 1
  %fv = extractvalue %EnumObj %fo, 1
  %ep = extractvalue %EnumObj %eo, 2
  %fp_ = extractvalue %EnumObj %fo, 2
  %ncm = call i32 @strcmp(i8* %en, i8* %fn_)
  %neq2 = icmp eq i32 %ncm, 0
  %vcm = call i32 @strcmp(i8* %ev, i8* %fv)
  %veq = icmp eq i32 %vcm, 0
  %nveq = and i1 %neq2, %veq
  br i1 %nveq, label %payload, label %ret_false
payload:
  %anull = icmp eq %Value* %ep, null
  %bnull = icmp eq %Value* %fp_, null
  %bothn = and i1 %anull, %bnull
  br i1 %bothn, label %ret_true, label %somechk
somechk:
  %same = icmp eq i1 %anull, %bnull
  br i1 %same, label %pl_cmp, label %ret_false
pl_cmp:
  %pv = load %Value, %Value* %ep
  %qv = load %Value, %Value* %fp_
  %peq = call i1 @hc_eq(%Value %pv, %Value %qv)
  ret i1 %peq
ret_false:
  ret i1 false
ret_true:
  ret i1 true
}
"#;

/// P11d [continuous] 递归深拷贝（对齐 oracle `deep_copy` `interp.rs:5651-5695`）：
/// Arr/Class 逐元素/字段递归拷贝；Ptr 新建 cell 拷贝所指值；Str 原生为不可变全局
/// 字符串（拷贝指针即等价，无 Rc 生命周期），恒等。Closure 原生不存在
/// （`MakeClosure` 硬拒绝），其余（标量/枚举/End 等）按值恒等。
const HC_DEEP_COPY: &str = r#"define %Value @hc_deep_copy(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %chk_cls
chk_cls:
  %is_cls = icmp eq i32 %t, 10
  br i1 %is_cls, label %cls, label %chk_str
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %id, label %chk_ptr
chk_ptr:
  %is_ptr = icmp eq i32 %t, 7
  br i1 %is_ptr, label %ptr, label %id
arr:
  %ad = extractvalue %Value %v, 1
  %aop = inttoptr i128 %ad to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %aop
  %alen = extractvalue %ArrObj %ao, 0
  %abase = extractvalue %ArrObj %ao, 1
  %anew = call %Value @hc_make_arr(i64 %alen)
  br label %aloop
aloop:
  %i = phi i64 [ 0, %arr ], [ %i2, %anext ]
  %idone = icmp uge i64 %i, %alen
  br i1 %idone, label %aret, label %abody
abody:
  %ap = getelementptr %Value, %Value* %abase, i64 %i
  %av = load %Value, %Value* %ap
  %ac = call %Value @hc_deep_copy(%Value %av)
  call void @hc_arr_set(%Value %anew, i64 %i, %Value %ac)
  br label %anext
anext:
  %i2 = add i64 %i, 1
  br label %aloop
aret:
  ret %Value %anew
cls:
  %cd = extractvalue %Value %v, 1
  %cop = inttoptr i128 %cd to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %cn = extractvalue %ClassObj %co, 0
  %ccnt = extractvalue %ClassObj %co, 1
  %cfs = extractvalue %ClassObj %co, 2
  %cnew = call %Value @hc_make_class(i8* %cn, i64 %ccnt)
  br label %cloop
cloop:
  %j = phi i64 [ 0, %cls ], [ %j2, %cnext ]
  %jdone = icmp uge i64 %j, %ccnt
  br i1 %jdone, label %cret, label %cbody
cbody:
  %fp = getelementptr %Field, %Field* %cfs, i64 %j
  %f = load %Field, %Field* %fp
  %fname = extractvalue %Field %f, 0
  %fval = extractvalue %Field %f, 1
  %fcp = call %Value @hc_deep_copy(%Value %fval)
  call void @hc_class_set(%Value %cnew, i64 %j, i8* %fname, %Value %fcp)
  br label %cnext
cnext:
  %j2 = add i64 %j, 1
  br label %cloop
cret:
  ret %Value %cnew
ptr:
  %pd = extractvalue %Value %v, 1
  %pop = inttoptr i128 %pd to %Value*
  %pv = load %Value, %Value* %pop
  %pc = call %Value @hc_deep_copy(%Value %pv)
  %sz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %sz)
  %ncell = bitcast i8* %raw to %Value*
  store %Value %pc, %Value* %ncell
  %np = ptrtoint %Value* %ncell to i128
  %nv0 = insertvalue %Value { i32 0, i128 0 }, i32 7, 0
  %nv1 = insertvalue %Value %nv0, i128 %np, 1
  ret %Value %nv1
id:
  ret %Value %v
}
"#;

/// P11d [continuous] DeepCopy 运行时门 `hc_deep_copy_cont`：值非 Class 恒等；
/// Class 名 ∈ `module.continuous` → `hc_deep_copy` 递归拷贝；否则恒等
/// （对齐 oracle `type_is_continuous` 仅对 Named 连续类生效，非连续类/数组 = 引用别名）。
/// LLVM 18 禁止常量表达式 GEP 于全局初始化器——名检查以 strcmp 链内联（同方法分派模式）。
fn emit_deep_copy_gate(out: &mut String, strings: &[String], continuous: &HashSet<String>) {
    let mut names: Vec<&String> = continuous.iter().collect();
    names.sort();
    // 无连续类：恒等门（`needs_deep_copy` 未发射指令，保守兜底），不引入递归 helper
    if names.is_empty() {
        out.push_str(
            "define %Value @hc_deep_copy_cont(%Value %v) {\nentry:\n  ret %Value %v\n}\n\n",
        );
        return;
    }
    out.push_str(HC_DEEP_COPY);
    out.push('\n');
    let mut b = String::new();
    b.push_str("define %Value @hc_deep_copy_cont(%Value %v) {\n");
    b.push_str("entry:\n");
    b.push_str("  %t = extractvalue %Value %v, 0\n");
    b.push_str("  %is_cls = icmp eq i32 %t, 10\n");
    b.push_str("  br i1 %is_cls, label %getname, label %id\n");
    b.push_str("getname:\n");
    b.push_str("  %d = extractvalue %Value %v, 1\n");
    b.push_str("  %op = inttoptr i128 %d to %ClassObj*\n");
    b.push_str("  %co = load %ClassObj, %ClassObj* %op\n");
    b.push_str("  %cname = extractvalue %ClassObj %co, 0\n");
    b.push_str("  br label %cmp0\n");
    for (i, name) in names.iter().enumerate() {
        let (si, sn) = str_idx(strings, name);
        b.push_str(&format!("cmp{i}:\n"));
        b.push_str(&format!(
            "  %g{i} = getelementptr inbounds [{sn} x i8], ptr @.str.{si}, i64 0, i64 0\n"
        ));
        b.push_str(&format!(
            "  %c{i} = call i32 @strcmp(i8* %cname, i8* %g{i})\n"
        ));
        b.push_str(&format!("  %e{i} = icmp eq i32 %c{i}, 0\n"));
        if i + 1 < names.len() {
            b.push_str(&format!(
                "  br i1 %e{i}, label %copy, label %cmp{}\n",
                i + 1
            ));
        } else {
            b.push_str(&format!("  br i1 %e{i}, label %copy, label %id\n"));
        }
    }
    b.push_str("copy:\n");
    b.push_str("  %c = call %Value @hc_deep_copy(%Value %v)\n");
    b.push_str("  ret %Value %c\n");
    b.push_str("id:\n");
    b.push_str("  ret %Value %v\n");
    b.push_str("}\n\n");
    out.push_str(&b);
}

fn emit_aggregate_helpers(out: &mut String) {
    // 聚合 tag 常量与模板字符串内的字面量保持一致（标记使用，防 dead-code 告警）。
    let _ = [T_ARR, T_SLICE, T_CLASS, T_ENUM, T_END];
    for h in [
        HC_ALLOC,
        HC_MAKE_ARR,
        HC_ARR_SET,
        HC_APPEND,
        HC_APPEND_U64,
        HC_EXTEND,
        HC_MAKE_CLASS,
        HC_CLASS_SET,
        HC_MAKE_ENUM,
        HC_UNWRAP,
        HC_INDEX,
        HC_STORE_INDEX,
        HC_SLICE,
        HC_STORE_SLICE,
        HC_CLASS_FIND,
        HC_FIELD,
        HC_STORE_FIELD,
        HC_SEQ_INFO,
        HC_EQ_AGG,
    ] {
        out.push_str(h);
        out.push('\n');
    }
}

// ---------- Phase 3 switch helper（模式匹配 / 枚举负载捕获） ----------

const HC_MATCH_TEST: &str = r#"define %Value @hc_match_test(%Value %subj, i8 %tag, i128 %data, i8* %str, i64 %len) {
entry:
  %slot = alloca %Value, align 8
  %f0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  store %Value %f0, %Value* %slot
  %b = call %Value @hc_deref(%Value %subj)
  %t = extractvalue %Value %b, 0
  %d = extractvalue %Value %b, 1
  switch i8 %tag, label %done [
    i8 0, label %t_err
    i8 1, label %t_ident
    i8 2, label %t_int
    i8 3, label %t_float
    i8 4, label %t_str
    i8 5, label %t_char
  ]
t_err:
  %is_err = icmp eq i32 %t, 6
  br i1 %is_err, label %err_cmp, label %done
err_cmp:
  %eqc = icmp eq i128 %d, %data
  br label %store_res
t_ident:
  %is_bool = icmp eq i32 %t, 4
  br i1 %is_bool, label %ident_bool, label %chk_null
ident_bool:
  %truep = getelementptr inbounds [5 x i8], ptr @.hc_true, i64 0, i64 0
  %c1 = call i32 @strcmp(i8* %str, i8* %truep)
  %is_true = icmp eq i32 %c1, 0
  br i1 %is_true, label %bool_true, label %chk_false
bool_true:
  %eqt = icmp eq i128 %d, 1
  br label %store_res
chk_false:
  %falsep = getelementptr inbounds [6 x i8], ptr @.hc_false, i64 0, i64 0
  %c2 = call i32 @strcmp(i8* %str, i8* %falsep)
  %is_false = icmp eq i32 %c2, 0
  br i1 %is_false, label %bool_false, label %chk_null
bool_false:
  %eqf = icmp eq i128 %d, 0
  br label %store_res
chk_null:
  %is_null = icmp eq i32 %t, 1
  br i1 %is_null, label %null_cmp, label %chk_enum
null_cmp:
  %nullp = getelementptr inbounds [5 x i8], ptr @.hc_null, i64 0, i64 0
  %c3 = call i32 @strcmp(i8* %str, i8* %nullp)
  %eqn = icmp eq i32 %c3, 0
  br label %store_res
chk_enum:
  %is_enum = icmp eq i32 %t, 11
  br i1 %is_enum, label %enum_cmp, label %done
enum_cmp:
  %d2 = extractvalue %Value %b, 1
  %op = inttoptr i128 %d2 to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %op
  %vn = extractvalue %EnumObj %eo, 1
  %c4 = call i32 @strcmp(i8* %vn, i8* %str)
  %eqv = icmp eq i32 %c4, 0
  br label %store_res
t_int:
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %int_cmp, label %done
int_cmp:
  %eqi = icmp eq i128 %d, %data
  br label %store_res
t_float:
  %is_float = icmp eq i32 %t, 3
  br i1 %is_float, label %float_cmp, label %done
float_cmp:
  %eqf2 = icmp eq i128 %d, %data
  br label %store_res
t_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %str_cmp, label %done
str_cmp:
  %sp = inttoptr i128 %d to i8*
  %mc = call i32 @memcmp(i8* %str, i8* %sp, i64 %len)
  %eqs = icmp eq i32 %mc, 0
  br label %store_res
t_char:
  %is_int2 = icmp eq i32 %t, 2
  br i1 %is_int2, label %char_cmp, label %done
char_cmp:
  %eqc2 = icmp eq i128 %d, %data
  br label %store_res
store_res:
  %res = phi i1 [ %eqc, %err_cmp ], [ %eqt, %bool_true ], [ %eqf, %bool_false ], [ %eqn, %null_cmp ], [ %eqv, %enum_cmp ], [ %eqi, %int_cmp ], [ %eqf2, %float_cmp ], [ %eqs, %str_cmp ], [ %eqc2, %char_cmp ]
  %d3 = zext i1 %res to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 4, 0
  %v1 = insertvalue %Value %v0, i128 %d3, 1
  store %Value %v1, %Value* %slot
  br label %done
done:
  %rv = load %Value, %Value* %slot
  ret %Value %rv
}
"#;

const HC_ENUM_PAYLOAD: &str = r#"define %Value @hc_enum_payload(%Value %v) {
entry:
  %b = call %Value @hc_deref(%Value %v)
  %t = extractvalue %Value %b, 0
  %is_enum = icmp eq i32 %t, 11
  br i1 %is_enum, label %enum_, label %identity
enum_:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %op
  %pp = extractvalue %EnumObj %eo, 2
  %isnull = icmp eq %Value* %pp, null
  br i1 %isnull, label %identity, label %payload
payload:
  %pv = load %Value, %Value* %pp
  ret %Value %pv
identity:
  ret %Value %b
}
"#;

/// `lo..hi` → Arr（[lo, hi) 元素，对齐 run_ir `MakeRange`；lo ≥ hi → 空数组）。
const HC_MAKE_RANGE: &str = r#"define %Value @hc_make_range(%Value %lo, %Value %hi) {
entry:
  %l = call %Value @hc_deref(%Value %lo)
  %lt = extractvalue %Value %l, 0
  %lt2 = icmp eq i32 %lt, 2
  br i1 %lt2, label %l_ok, label %typeerr
l_ok:
  %h = call %Value @hc_deref(%Value %hi)
  %ht = extractvalue %Value %h, 0
  %ht2 = icmp eq i32 %ht, 2
  br i1 %ht2, label %h_ok, label %typeerr
h_ok:
  %lv = extractvalue %Value %l, 1
  %hv = extractvalue %Value %h, 1
  %sub = sub i128 %hv, %lv
  %neg = icmp slt i128 %sub, 0
  %cnt = select i1 %neg, i128 0, i128 %sub
  %cnt64 = trunc i128 %cnt to i64
  %arr = call %Value @hc_make_arr(i64 %cnt64)
  br label %loop
loop:
  %i = phi i64 [ 0, %h_ok ], [ %i2, %next ]
  %done = icmp uge i64 %i, %cnt64
  br i1 %done, label %fin, label %body
body:
  %vi = zext i64 %i to i128
  %lv2 = add i128 %lv, %vi
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %lv2, 1
  call void @hc_arr_set(%Value %arr, i64 %i, %Value %v1)
  br label %next
next:
  %i2 = add i64 %i, 1
  br label %loop
typeerr:
  call void @hc_abort_typeerr()
  unreachable
fin:
  ret %Value %arr
}
"#;

fn emit_switch_helpers(out: &mut String) {
    out.push_str(HC_MATCH_TEST);
    out.push('\n');
    out.push_str(HC_ENUM_PAYLOAD);
    out.push('\n');
    out.push_str(HC_MAKE_RANGE);
    out.push('\n');
}

// ---------- Phase 3 for 迭代器 helper（Arr/Slice 共享源指针写回；Str/Map 新单元） ----------

const HC_ITER_ALLOC: &str = r#"define %IterObj* @hc_iter_alloc(i64 %n) {
entry:
  %osz = ptrtoint %IterObj* getelementptr (%IterObj, %IterObj* null, i32 1) to i64
  %oraw = call i8* @hc_alloc(i64 %osz)
  %op = bitcast i8* %oraw to %IterObj*
  %isz = ptrtoint %IterItemObj* getelementptr (%IterItemObj, %IterItemObj* null, i32 1) to i64
  %is = mul i64 %isz, %n
  %iraw = call i8* @hc_alloc(i64 %is)
  %ip = bitcast i8* %iraw to %IterItemObj*
  %o0 = insertvalue %IterObj undef, %IterItemObj* %ip, 0
  %o1 = insertvalue %IterObj %o0, i64 %n, 1
  %o2 = insertvalue %IterObj %o1, i64 0, 2
  %o3 = insertvalue %IterObj %o2, i64 -1, 3
  store %IterObj %o3, %IterObj* %op
  ret %IterObj* %op
}
"#;

const HC_ITER_SET: &str = r#"define void @hc_iter_set(%IterObj* %iter, i64 %i, %Value* %src, i1 %is_ref) {
entry:
  %it = load %IterObj, %IterObj* %iter
  %items = extractvalue %IterObj %it, 0
  %ip = getelementptr %IterItemObj, %IterItemObj* %items, i64 %i
  %it0 = insertvalue %IterItemObj undef, %Value* %src, 0
  %it1 = insertvalue %IterItemObj %it0, i1 %is_ref, 1
  store %IterItemObj %it1, %IterItemObj* %ip
  ret void
}
"#;

/// 取下一项：`has`（Bool）写入迭代器当前项值副本到捕获槽；无下一项 → false。
/// Mut/Move 捕获由 `hc_iter_write_back` 在循环体末尾写回（拷贝进出——LLVM 槽模型无
/// 单元间接层；run_ir 侧槽 cell 即源 cell，写回为无操作）。
const HC_ITER_NEXT: &str = r#"define %Value @hc_iter_next(%IterObj* %iter, %Value* %slot) {
entry:
  %it = load %IterObj, %IterObj* %iter
  %count = extractvalue %IterObj %it, 1
  %next = extractvalue %IterObj %it, 2
  %done = icmp uge i64 %next, %count
  br i1 %done, label %ret_false, label %got
got:
  %items = extractvalue %IterObj %it, 0
  %ip = getelementptr %IterItemObj, %IterItemObj* %items, i64 %next
  %item = load %IterItemObj, %IterItemObj* %ip
  %src = extractvalue %IterItemObj %item, 0
  %val = load %Value, %Value* %src
  store %Value %val, %Value* %slot
  %i0 = insertvalue %IterObj %it, i64 %next, 3
  %next2 = add i64 %next, 1
  %i1 = insertvalue %IterObj %i0, i64 %next2, 2
  store %IterObj %i1, %IterObj* %iter
  ret %Value { i32 4, i128 1 }
ret_false:
  ret %Value { i32 4, i128 0 }
}
"#;

const HC_ITER_WRITE_BACK: &str = r#"define void @hc_iter_write_back(%IterObj* %iter, %Value* %slot) {
entry:
  %it = load %IterObj, %IterObj* %iter
  %wb = extractvalue %IterObj %it, 3
  %none = icmp eq i64 %wb, -1
  br i1 %none, label %done, label %wb2
wb2:
  %items = extractvalue %IterObj %it, 0
  %ip = getelementptr %IterItemObj, %IterItemObj* %items, i64 %wb
  %item = load %IterItemObj, %IterItemObj* %ip
  %src = extractvalue %IterItemObj %item, 0
  %val = load %Value, %Value* %slot
  store %Value %val, %Value* %src
  br label %done
done:
  ret void
}
"#;

/// 展开可迭代值（对齐 run_ir `make_iter`）：Arr/Slice 元素源指针 `is_ref=true`（写穿）；
/// Str → 字节新单元；Map → KV 类新单元（key 字段 + value 字段副本，`is_ref=false`——
/// 原生 Map 写回为 KV 副本，收敛于 Phase 7 标准库）；其它 Class（用户 IIterable）→
/// NotIterable 硬错误（方法动态分派属 Phase 4）。
const HC_ITER_MAKE: &str = r#"define %Value @hc_iter_make(%Value %base) {
entry:
  %b = call %Value @hc_deref(%Value %base)
  %t = extractvalue %Value %b, 0
  %is_arr = icmp eq i32 %t, 8
  br i1 %is_arr, label %arr, label %chk_slice
arr:
  %d = extractvalue %Value %b, 1
  %op = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %op
  %cnt = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %iter = call %IterObj* @hc_iter_alloc(i64 %cnt)
  br label %arr_loop
arr_loop:
  %i = phi i64 [ 0, %arr ], [ %i2, %arr_next ]
  %adone = icmp uge i64 %i, %cnt
  br i1 %adone, label %fin_arr, label %arr_body
arr_body:
  %asrc = getelementptr %Value, %Value* %items, i64 %i
  call void @hc_iter_set(%IterObj* %iter, i64 %i, %Value* %asrc, i1 true)
  br label %arr_next
arr_next:
  %i2 = add i64 %i, 1
  br label %arr_loop
fin_arr:
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 13, 0
  %ip0 = ptrtoint %IterObj* %iter to i128
  %v1 = insertvalue %Value %v0, i128 %ip0, 1
  ret %Value %v1
chk_slice:
  %is_slc = icmp eq i32 %t, 9
  br i1 %is_slc, label %slc, label %chk_str
slc:
  %d3 = extractvalue %Value %b, 1
  %sop = inttoptr i128 %d3 to %SliceObj*
  %so = load %SliceObj, %SliceObj* %sop
  %sdata = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %iter3 = call %IterObj* @hc_iter_alloc(i64 %slen)
  br label %slc_loop
slc_loop:
  %k = phi i64 [ 0, %slc ], [ %k2, %slc_next ]
  %sdone = icmp uge i64 %k, %slen
  br i1 %sdone, label %fin_slc, label %slc_body
slc_body:
  %sidx = add i64 %sstart, %k
  %ssrc = getelementptr %Value, %Value* %sdata, i64 %sidx
  call void @hc_iter_set(%IterObj* %iter3, i64 %k, %Value* %ssrc, i1 true)
  br label %slc_next
slc_next:
  %k2 = add i64 %k, 1
  br label %slc_loop
fin_slc:
  %u0 = insertvalue %Value { i32 0, i128 0 }, i32 13, 0
  %uip = ptrtoint %IterObj* %iter3 to i128
  %u1 = insertvalue %Value %u0, i128 %uip, 1
  ret %Value %u1
chk_str:
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %str_, label %chk_class
str_:
  %d2 = extractvalue %Value %b, 1
  %sp2 = inttoptr i128 %d2 to i8*
  %blen = call i64 @strlen(i8* %sp2)
  %iter2 = call %IterObj* @hc_iter_alloc(i64 %blen)
  br label %str_loop
str_loop:
  %j = phi i64 [ 0, %str_ ], [ %j2, %str_next ]
  %bdone = icmp uge i64 %j, %blen
  br i1 %bdone, label %fin_str, label %str_body
str_body:
  %cp = getelementptr i8, i8* %sp2, i64 %j
  %c = load i8, i8* %cp
  %ci = zext i8 %c to i128
  %bv0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %bv1 = insertvalue %Value %bv0, i128 %ci, 1
  %vsz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %craw = call i8* @hc_alloc(i64 %vsz)
  %cell = bitcast i8* %craw to %Value*
  store %Value %bv1, %Value* %cell
  call void @hc_iter_set(%IterObj* %iter2, i64 %j, %Value* %cell, i1 false)
  br label %str_next
str_next:
  %j2 = add i64 %j, 1
  br label %str_loop
fin_str:
  %w0 = insertvalue %Value { i32 0, i128 0 }, i32 13, 0
  %wip = ptrtoint %IterObj* %iter2 to i128
  %w1 = insertvalue %Value %w0, i128 %wip, 1
  ret %Value %w1
chk_class:
  %is_cls = icmp eq i32 %t, 10
  br i1 %is_cls, label %cls, label %notiter
cls:
  %d4 = extractvalue %Value %b, 1
  %cop = inttoptr i128 %d4 to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %tyn = extractvalue %ClassObj %co, 0
  %mapp = getelementptr inbounds [4 x i8], ptr @.hc_map, i64 0, i64 0
  %mcmp = call i32 @strcmp(i8* %tyn, i8* %mapp)
  %is_map = icmp eq i32 %mcmp, 0
  br i1 %is_map, label %map_, label %notiter
map_:
  %mcnt = extractvalue %ClassObj %co, 1
  %mfields = extractvalue %ClassObj %co, 2
  %iter4 = call %IterObj* @hc_iter_alloc(i64 %mcnt)
  br label %map_loop
map_loop:
  %m = phi i64 [ 0, %map_ ], [ %m2, %map_next ]
  %mdone = icmp uge i64 %m, %mcnt
  br i1 %mdone, label %fin_map, label %map_body
map_body:
  %mfp = getelementptr %Field, %Field* %mfields, i64 %m
  %mf = load %Field, %Field* %mfp
  %mfn = extractvalue %Field %mf, 0
  %mfv = extractvalue %Field %mf, 1
  ; KV 类：key = 字段名 Str，value = 字段值副本（is_ref=false）
  %kvp = getelementptr inbounds [3 x i8], ptr @.hc_kv, i64 0, i64 0
  %kv = call %Value @hc_make_class(i8* %kvp, i64 2)
  %keyp = getelementptr inbounds [4 x i8], ptr @.hc_key, i64 0, i64 0
  %mfnptr = ptrtoint i8* %mfn to i128
  %ks0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %ks1 = insertvalue %Value %ks0, i128 %mfnptr, 1
  call void @hc_class_set(%Value %kv, i64 0, i8* %keyp, %Value %ks1)
  %valp = getelementptr inbounds [6 x i8], ptr @.hc_value, i64 0, i64 0
  call void @hc_class_set(%Value %kv, i64 1, i8* %valp, %Value %mfv)
  ; 每项独立 cell 持有 KV 副本（写回不传播到源 Map——与 oracle KV 新 cell 一致）
  %vsz2 = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %kraw = call i8* @hc_alloc(i64 %vsz2)
  %kcell = bitcast i8* %kraw to %Value*
  store %Value %kv, %Value* %kcell
  call void @hc_iter_set(%IterObj* %iter4, i64 %m, %Value* %kcell, i1 false)
  br label %map_next
map_next:
  %m2 = add i64 %m, 1
  br label %map_loop
fin_map:
  %x0 = insertvalue %Value { i32 0, i128 0 }, i32 13, 0
  %xip = ptrtoint %IterObj* %iter4 to i128
  %x1 = insertvalue %Value %x0, i128 %xip, 1
  ret %Value %x1
notiter:
  call void @hc_abort_notiter()
  unreachable
}
"#;

fn emit_iter_helpers(out: &mut String) {
    for h in [
        HC_ITER_ALLOC,
        HC_ITER_SET,
        HC_ITER_NEXT,
        HC_ITER_WRITE_BACK,
        HC_ITER_MAKE,
    ] {
        out.push_str(h);
        out.push('\n');
    }
}

// ---------- Phase 7 io.print 显示 helper（定长字节写 + 值格式化） ----------
//
// 取舍（P7e 子集 + B3）：`hc_write_value` 对 Int/Float/Bool/Str/Null/Err/Arr/Slice/Class/Enum
// 对齐 oracle `display`；`{x}/{X}/{b}` 对 Int 走 16/16大写/2 进制（负数按无符号位模式，对齐 Rust
// `{n:x}/{n:X}/{n:b}`）；`{e}` 对 Float 走 printf `%e`（固定 6 位小数 + 带符号指数，与 Rust 最短
// 表示的 `{f:e}` 有差异——记录为已知取舍，`{e}` 无示例/测试比对精确输出）。Err 显示为 `error.{code}`
// （原生无错误名表——与 IR 的 `error.{name}` 有差异，记录为已知取舍）。Float 用 `%.1f`（整值）/
// `%.15g`（余值）近似 Rust 最短表示。

const HC_WRITE_BYTES: &str = r#"define void @hc_write_bytes(i8* %p, i64 %n) {
entry:
  %n32 = trunc i64 %n to i32
  %f = getelementptr inbounds [5 x i8], ptr @.fmt_pct, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %f, i32 %n32, i8* %p)
  ret void
}
"#;

const HC_WRITE_STRZ: &str = r#"define void @hc_write_strz(i8* %p) {
entry:
  %f = getelementptr inbounds [3 x i8], ptr @.fmt_s, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %f, i8* %p)
  ret void
}
"#;

const HC_WRITE_U128_BASE: &str = r#"define void @hc_write_u128_base(i128 %v, i32 %base, i32 %upper) {
entry:
  %buf = alloca [64 x i8], align 1
  %b64 = zext i32 %base to i128
  %up_i1 = icmp ne i32 %upper, 0
  br label %loop
loop:
  %val = phi i128 [ %v, %entry ], [ %quot, %loop ]
  %pos = phi i64 [ 63, %entry ], [ %dec, %loop ]
  %quot = udiv i128 %val, %b64
  %rem = urem i128 %val, %b64
  %rem64 = trunc i128 %rem to i64
  %lt10 = icmp ult i64 %rem64, 10
  %dig = add i64 %rem64, 48
  %off = select i1 %up_i1, i64 55, i64 87
  %up = add i64 %rem64, %off
  %ch64 = select i1 %lt10, i64 %dig, i64 %up
  %ch = trunc i64 %ch64 to i8
  %dst = getelementptr inbounds [64 x i8], ptr %buf, i64 0, i64 %pos
  store i8 %ch, i8* %dst
  %dec = sub i64 %pos, 1
  %is0 = icmp eq i128 %quot, 0
  br i1 %is0, label %write, label %loop
write:
  %sp = getelementptr inbounds [64 x i8], ptr %buf, i64 0, i64 %pos
  %len = sub i64 64, %pos
  call void @hc_write_bytes(i8* %sp, i64 %len)
  ret void
}
"#;

const HC_WRITE_I128_DEC: &str = r#"define void @hc_write_i128_dec(i128 %n) {
entry:
  %is_neg = icmp slt i128 %n, 0
  br i1 %is_neg, label %neg, label %mag
neg:
  %dash = getelementptr inbounds [2 x i8], ptr @.hc_dash, i64 0, i64 0
  call void @hc_write_bytes(i8* %dash, i64 1)
  br label %mag
mag:
  %negv = sub i128 0, %n
  %m = select i1 %is_neg, i128 %negv, i128 %n
  call void @hc_write_u128_base(i128 %m, i32 10, i32 0)
  ret void
}
"#;

const HC_WRITE_INT: &str = r#"define void @hc_write_int(%Value %v, i32 %mode) {
entry:
  %d = extractvalue %Value %v, 1
  %m0 = icmp eq i32 %mode, 0
  %m3 = icmp eq i32 %mode, 3
  %is_dec = or i1 %m0, %m3
  br i1 %is_dec, label %dec, label %basefmt
dec:
  call void @hc_write_i128_dec(i128 %d)
  ret void
basefmt:
  %is_bin = icmp eq i32 %mode, 2
  %is_hex = icmp eq i32 %mode, 1
  %is_hexu = icmp eq i32 %mode, 4
  %is_hex2 = or i1 %is_hex, %is_hexu
  %b1 = select i1 %is_hex2, i32 16, i32 10
  %base = select i1 %is_bin, i32 2, i32 %b1
  %upper = select i1 %is_hexu, i32 1, i32 0
  call void @hc_write_u128_base(i128 %d, i32 %base, i32 %upper)
  ret void
}
"#;

const HC_WRITE_TYPENAME: &str = r#"define void @hc_write_typename(%Value %v) {
entry:
  %tag = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %tc = icmp eq i32 %tag, 10
  br i1 %tc, label %cls, label %t_en
t_en:
  %te = icmp eq i32 %tag, 11
  br i1 %te, label %en, label %t2
cls:
  %cop = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %tn = extractvalue %ClassObj %co, 0
  call void @hc_write_strz(i8* %tn)
  ret void
en:
  %eop = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %eop
  %n = extractvalue %EnumObj %eo, 0
  call void @hc_write_strz(i8* %n)
  ret void
t2:
  %t2c = icmp eq i32 %tag, 2
  br i1 %t2c, label %n_i128, label %t3
t3:
  %t3c = icmp eq i32 %tag, 3
  br i1 %t3c, label %n_f64, label %t4
t4:
  %t4c = icmp eq i32 %tag, 4
  br i1 %t4c, label %n_bool, label %t5
t5:
  %t5c = icmp eq i32 %tag, 5
  br i1 %t5c, label %n_str, label %t8
t8:
  %t8c = icmp eq i32 %tag, 8
  br i1 %t8c, label %n_arr, label %t9
t9:
  %t9c = icmp eq i32 %tag, 9
  br i1 %t9c, label %n_slice, label %t1
t1:
  %t1c = icmp eq i32 %tag, 1
  br i1 %t1c, label %n_opt, label %t6
t6:
  %t6c = icmp eq i32 %tag, 6
  br i1 %t6c, label %n_err, label %t7
t7:
  %t7c = icmp eq i32 %tag, 7
  br i1 %t7c, label %n_ptr, label %t14
t14:
  %t14c = icmp eq i32 %tag, 14
  br i1 %t14c, label %n_fn, label %t15
t15:
  %t15c = icmp eq i32 %tag, 15
  br i1 %t15c, label %n_closure, label %t12
t12:
  %t12c = icmp eq i32 %tag, 12
  br i1 %t12c, label %n_end, label %t13
t13:
  %t13c = icmp eq i32 %tag, 13
  br i1 %t13c, label %n_iter, label %n_void
n_i128:
  call void @hc_write_strz(i8* @.t_i128)
  ret void
n_f64:
  call void @hc_write_strz(i8* @.t_f64)
  ret void
n_bool:
  call void @hc_write_strz(i8* @.t_bool)
  ret void
n_str:
  call void @hc_write_strz(i8* @.t_str)
  ret void
n_arr:
  call void @hc_write_strz(i8* @.t_arr)
  ret void
n_slice:
  call void @hc_write_strz(i8* @.t_slice)
  ret void
n_opt:
  call void @hc_write_strz(i8* @.t_opt)
  ret void
n_err:
  call void @hc_write_strz(i8* @.t_err)
  ret void
n_ptr:
  call void @hc_write_strz(i8* @.t_ptr)
  ret void
n_fn:
  call void @hc_write_strz(i8* @.t_fn)
  ret void
n_closure:
  call void @hc_write_strz(i8* @.t_closure)
  ret void
n_end:
  call void @hc_write_strz(i8* @.t_end)
  ret void
n_iter:
  call void @hc_write_strz(i8* @.t_iter)
  ret void
n_void:
  call void @hc_write_strz(i8* @.t_void)
  ret void
}
"#;

const HC_WRITE_VALUE: &str = r#"define void @hc_write_value(%Value %v, i32 %mode) {
entry:
  %tag = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %ti = icmp eq i32 %tag, 2
  br i1 %ti, label %int_case, label %ck_f
ck_f:
  %tf = icmp eq i32 %tag, 3
  br i1 %tf, label %float_case, label %ck_b
ck_b:
  %tb = icmp eq i32 %tag, 4
  br i1 %tb, label %bool_case, label %ck_s
ck_s:
  %ts = icmp eq i32 %tag, 5
  br i1 %ts, label %str_case, label %ck_n
ck_n:
  %tn = icmp eq i32 %tag, 1
  br i1 %tn, label %null_case, label %ck_e
ck_e:
  %te = icmp eq i32 %tag, 6
  br i1 %te, label %err_case, label %ck_a
ck_a:
  %ta = icmp eq i32 %tag, 8
  br i1 %ta, label %arr_case, label %ck_sl
ck_sl:
  %tsl = icmp eq i32 %tag, 9
  br i1 %tsl, label %slice_case, label %ck_c
ck_c:
  %tc = icmp eq i32 %tag, 10
  br i1 %tc, label %class_case, label %ck_en
ck_en:
  %ten = icmp eq i32 %tag, 11
  br i1 %ten, label %enum_case, label %ck_p
ck_p:
  %tp = icmp eq i32 %tag, 7
  br i1 %tp, label %ptr_case, label %fallback
fallback:
  call void @hc_write_typename(%Value %v)
  ret void
int_case:
  call void @hc_write_int(%Value %v, i32 %mode)
  ret void
float_case:
  %me5 = icmp eq i32 %mode, 5
  br i1 %me5, label %exp_f, label %norm_f
exp_f:
  %dt2 = trunc i128 %d to i64
  %fve = bitcast i64 %dt2 to double
  %pe = getelementptr inbounds [3 x i8], ptr @.fmt_e, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %pe, double %fve)
  ret void
norm_f:
  %dt = trunc i128 %d to i64
  %fv = bitcast i64 %dt to double
  %fr = call double @fmod(double %fv, double 1.0)
  %fz = fcmp oeq double %fr, 0.0
  %fa = call double @fabs(double %fv)
  %fl = fcmp olt double %fa, 1.0e15
  %whole = and i1 %fz, %fl
  br i1 %whole, label %whole_f, label %frac_f
whole_f:
  %p1 = getelementptr inbounds [5 x i8], ptr @.fmt_one, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %p1, double %fv)
  ret void
frac_f:
  %pg = getelementptr inbounds [6 x i8], ptr @.fmt_g15, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %pg, double %fv)
  ret void
bool_case:
  %is_t = icmp eq i128 %d, 1
  %bp = select i1 %is_t, i8* @.hc_true, i8* @.hc_false
  call void @hc_write_strz(i8* %bp)
  ret void
str_case:
  %sp = trunc i128 %d to i64
  %pp = inttoptr i64 %sp to i8*
  %n = call i64 @strlen(i8* %pp)
  call void @hc_write_bytes(i8* %pp, i64 %n)
  ret void
null_case:
  call void @hc_write_strz(i8* @.hc_null)
  ret void
err_case:
  %ep = getelementptr inbounds [7 x i8], ptr @.hc_errpre, i64 0, i64 0
  call void @hc_write_bytes(i8* %ep, i64 6)
  call void @hc_write_i128_dec(i128 %d)
  ret void
ptr_case:
  %pd = call %Value @hc_deref(%Value %v)
  call void @hc_write_value(%Value %pd, i32 %mode)
  ret void
arr_case:
  %ap = inttoptr i128 %d to %ArrObj*
  %ao = load %ArrObj, %ArrObj* %ap
  %alen = extractvalue %ArrObj %ao, 0
  %items = extractvalue %ArrObj %ao, 1
  %lb = getelementptr inbounds [2 x i8], ptr @.hc_lb, i64 0, i64 0
  call void @hc_write_bytes(i8* %lb, i64 1)
  br label %aloop
aloop:
  %i = phi i64 [ 0, %arr_case ], [ %inext, %aelem ]
  %gt0 = icmp ugt i64 %i, 0
  br i1 %gt0, label %acomma, label %aelem
acomma:
  %cma = getelementptr inbounds [3 x i8], ptr @.hc_comma, i64 0, i64 0
  call void @hc_write_bytes(i8* %cma, i64 2)
  br label %aelem
aelem:
  %ep2 = getelementptr %Value, %Value* %items, i64 %i
  %ev = load %Value, %Value* %ep2
  call void @hc_write_value(%Value %ev, i32 0)
  %inext = add i64 %i, 1
  %adone = icmp uge i64 %inext, %alen
  br i1 %adone, label %aend, label %aloop
aend:
  %rb = getelementptr inbounds [2 x i8], ptr @.hc_rb, i64 0, i64 0
  call void @hc_write_bytes(i8* %rb, i64 1)
  ret void
slice_case:
  %sop = inttoptr i128 %d to %SliceObj*
  %so = load %SliceObj, %SliceObj* %sop
  %sdata = extractvalue %SliceObj %so, 0
  %sstart = extractvalue %SliceObj %so, 1
  %slen = extractvalue %SliceObj %so, 2
  %lb2 = getelementptr inbounds [2 x i8], ptr @.hc_lb, i64 0, i64 0
  call void @hc_write_bytes(i8* %lb2, i64 1)
  br label %sloop
sloop:
  %si = phi i64 [ 0, %slice_case ], [ %sinext, %selem ]
  %sgt0 = icmp ugt i64 %si, 0
  br i1 %sgt0, label %scomma, label %selem
scomma:
  %cma2 = getelementptr inbounds [3 x i8], ptr @.hc_comma, i64 0, i64 0
  call void @hc_write_bytes(i8* %cma2, i64 2)
  br label %selem
selem:
  %sidx = add i64 %sstart, %si
  %sep = getelementptr %Value, %Value* %sdata, i64 %sidx
  %sev = load %Value, %Value* %sep
  call void @hc_write_value(%Value %sev, i32 0)
  %sinext = add i64 %si, 1
  %sdone = icmp uge i64 %sinext, %slen
  br i1 %sdone, label %send, label %sloop
send:
  %rb2 = getelementptr inbounds [2 x i8], ptr @.hc_rb, i64 0, i64 0
  call void @hc_write_bytes(i8* %rb2, i64 1)
  ret void
class_case:
  %cop = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %tyname = extractvalue %ClassObj %co, 0
  call void @hc_write_strz(i8* %tyname)
  %clen = extractvalue %ClassObj %co, 1
  %cfields = extractvalue %ClassObj %co, 2
  %bl = getelementptr inbounds [4 x i8], ptr @.hc_bra_l, i64 0, i64 0
  call void @hc_write_bytes(i8* %bl, i64 3)
  br label %cloop
cloop:
  %ci = phi i64 [ 0, %class_case ], [ %cinext, %celem ]
  %cgt0 = icmp ugt i64 %ci, 0
  br i1 %cgt0, label %ccomma, label %celem
ccomma:
  %cma3 = getelementptr inbounds [3 x i8], ptr @.hc_comma, i64 0, i64 0
  call void @hc_write_bytes(i8* %cma3, i64 2)
  br label %celem
celem:
  %cfp = getelementptr %Field, %Field* %cfields, i64 %ci
  %cfv = load %Field, %Field* %cfp
  %cfname = extractvalue %Field %cfv, 0
  %cfval = extractvalue %Field %cfv, 1
  %fnl = call i64 @strlen(i8* %cfname)
  call void @hc_write_bytes(i8* %cfname, i64 %fnl)
  %eqs = getelementptr inbounds [4 x i8], ptr @.hc_eqs, i64 0, i64 0
  call void @hc_write_bytes(i8* %eqs, i64 3)
  call void @hc_write_value(%Value %cfval, i32 0)
  %cinext = add i64 %ci, 1
  %cdone = icmp uge i64 %cinext, %clen
  br i1 %cdone, label %cend, label %cloop
cend:
  %brr = getelementptr inbounds [3 x i8], ptr @.hc_bra_r, i64 0, i64 0
  call void @hc_write_bytes(i8* %brr, i64 3)
  ret void
enum_case:
  %eop = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %eop
  %ename = extractvalue %EnumObj %eo, 0
  %evariant = extractvalue %EnumObj %eo, 1
  %epay = extractvalue %EnumObj %eo, 2
  call void @hc_write_strz(i8* %ename)
  %dot = getelementptr inbounds [2 x i8], ptr @.hc_dot, i64 0, i64 0
  call void @hc_write_bytes(i8* %dot, i64 1)
  call void @hc_write_strz(i8* %evariant)
  %is_none = icmp eq %Value* %epay, null
  br i1 %is_none, label %en_end, label %en_pay
en_pay:
  %eqs2 = getelementptr inbounds [4 x i8], ptr @.hc_eqs, i64 0, i64 0
  call void @hc_write_bytes(i8* %eqs2, i64 3)
  %pv = load %Value, %Value* %epay
  call void @hc_write_value(%Value %pv, i32 0)
  br label %en_end
en_end:
  ret void
}
"#;

fn emit_print_helpers(out: &mut String) {
    for h in [
        HC_WRITE_BYTES,
        HC_WRITE_STRZ,
        HC_WRITE_U128_BASE,
        HC_WRITE_I128_DEC,
        HC_WRITE_INT,
        HC_WRITE_TYPENAME,
        HC_WRITE_VALUE,
    ] {
        out.push_str(h);
        out.push('\n');
    }
}

// ---------- Phase 7 标量内建 helper（@sizeOf/@intCast/@typeOf/min/max/sqrt/box/copy/溢出） ----------

const HC_MIN: &str = r#"define %Value @hc_min(%Value %a, %Value %b) {
entry:
  %lt = call i1 @hc_lt(%Value %a, %Value %b)
  %r = select i1 %lt, %Value %a, %Value %b
  ret %Value %r
}
"#;

const HC_MAX: &str = r#"define %Value @hc_max(%Value %a, %Value %b) {
entry:
  %lt = call i1 @hc_lt(%Value %a, %Value %b)
  %r = select i1 %lt, %Value %b, %Value %a
  ret %Value %r
}
"#;

const HC_SQRT: &str = r#"define %Value @hc_sqrt(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_int = icmp eq i32 %t, 2
  %dt = trunc i128 %d to i64
  %asf = sitofp i64 %dt to double
  %raw = bitcast i64 %dt to double
  %f = select i1 %is_int, double %asf, double %raw
  %sq = call double @sqrt(double %f)
  %bits = bitcast double %sq to i64
  %z = zext i64 %bits to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 3, 0
  %v1 = insertvalue %Value %v0, i128 %z, 1
  ret %Value %v1
}
"#;

/// math 一元浮点 helper（对齐 oracle call_math interp.rs:4922-4960）：
/// Int → f64 强制 / Float 直用，应用 `op`（LLVM 计算行，结果在 `%r`）后返回 Float。
/// 与 `@hc_sqrt` 同构；`pow` 在 oracle 为 `f.powf(2.0)`（单参平方）。
fn math_unop_helper(fname: &str, op: &str) -> String {
    format!(
        r#"define %Value @{fname}(%Value %v) {{
entry:
  %t = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_int = icmp eq i32 %t, 2
  %dt = trunc i128 %d to i64
  %asf = sitofp i64 %dt to double
  %raw = bitcast i64 %dt to double
  %f = select i1 %is_int, double %asf, double %raw
  {op}
  %bits = bitcast double %r to i64
  %z = zext i64 %bits to i128
  %v0 = insertvalue %Value {{ i32 0, i128 0 }}, i32 3, 0
  %v1 = insertvalue %Value %v0, i128 %z, 1
  ret %Value %v1
}}
"#
    )
}

const HC_BOX: &str = r#"define %Value @hc_box(%Value %v) {
entry:
  %sz = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64
  %raw = call i8* @hc_alloc(i64 %sz)
  %cell = bitcast i8* %raw to %Value*
  store %Value %v, %Value* %cell
  %dp = ptrtoint %Value* %cell to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 7, 0
  %v1 = insertvalue %Value %v0, i128 %dp, 1
  ret %Value %v1
}
"#;

const HC_COPY: &str = r#"define %Value @hc_copy(%Value %v, %Value %mode) {
entry:
  %t = extractvalue %Value %mode, 0
  %is_enum = icmp eq i32 %t, 11
  br i1 %is_enum, label %chk, label %deep
chk:
  %d = extractvalue %Value %mode, 1
  %op = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %op
  %vn = extractvalue %EnumObj %eo, 1
  %sp = getelementptr inbounds [8 x i8], ptr @.hc_shallow, i64 0, i64 0
  %c = call i32 @strcmp(i8* %vn, i8* %sp)
  %is_sh = icmp eq i32 %c, 0
  br i1 %is_sh, label %id, label %deep
id:
  ret %Value %v
deep:
  %t2 = extractvalue %Value %v, 0
  %t8 = icmp eq i32 %t2, 8
  %t10 = icmp eq i32 %t2, 10
  %t7 = icmp eq i32 %t2, 7
  %t1 = icmp eq i32 %t2, 1
  %is_agg = or i1 %t8, %t10
  %is_agg2 = or i1 %is_agg, %t7
  %is_agg3 = or i1 %is_agg2, %t1
  br i1 %is_agg3, label %abort, label %id
abort:
  call void @hc_abort_builtin()
  unreachable
}
"#;

const HC_INTCAST: &str = r#"define %Value @hc_intcast(%Value %v, i128 %min, i128 %max) {
entry:
  %t = extractvalue %Value %v, 0
  %is_int = icmp eq i32 %t, 2
  br i1 %is_int, label %chk, label %abort
chk:
  %d = extractvalue %Value %v, 1
  %lo_ok = icmp sge i128 %d, %min
  %hi_ok = icmp sle i128 %d, %max
  %ok = and i1 %lo_ok, %hi_ok
  br i1 %ok, label %retv, label %abort
retv:
  ret %Value %v
abort:
  call void @hc_abort_intcast()
  unreachable
}
"#;

const HC_TYPEOF: &str = r#"define %Value @hc_typeof(%Value %v) {
entry:
  %tag = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %tc = icmp eq i32 %tag, 10
  br i1 %tc, label %cls, label %t_en
t_en:
  %te = icmp eq i32 %tag, 11
  br i1 %te, label %en, label %t2
cls:
  %cop = inttoptr i128 %d to %ClassObj*
  %co = load %ClassObj, %ClassObj* %cop
  %tn = extractvalue %ClassObj %co, 0
  br label %ret_str
en:
  %eop = inttoptr i128 %d to %EnumObj*
  %eo = load %EnumObj, %EnumObj* %eop
  %en_tn = extractvalue %EnumObj %eo, 0
  br label %ret_str
t2:
  %t2c = icmp eq i32 %tag, 2
  br i1 %t2c, label %n_i128, label %t3
t3:
  %t3c = icmp eq i32 %tag, 3
  br i1 %t3c, label %n_f64, label %t4
t4:
  %t4c = icmp eq i32 %tag, 4
  br i1 %t4c, label %n_bool, label %t5
t5:
  %t5c = icmp eq i32 %tag, 5
  br i1 %t5c, label %n_str, label %t8
t8:
  %t8c = icmp eq i32 %tag, 8
  br i1 %t8c, label %n_arr, label %t9
t9:
  %t9c = icmp eq i32 %tag, 9
  br i1 %t9c, label %n_slice, label %t1
t1:
  %t1c = icmp eq i32 %tag, 1
  br i1 %t1c, label %n_opt, label %t6
t6:
  %t6c = icmp eq i32 %tag, 6
  br i1 %t6c, label %n_err, label %t7
t7:
  %t7c = icmp eq i32 %tag, 7
  br i1 %t7c, label %n_ptr, label %t14
t14:
  %t14c = icmp eq i32 %tag, 14
  br i1 %t14c, label %n_fn, label %t15
t15:
  %t15c = icmp eq i32 %tag, 15
  br i1 %t15c, label %n_closure, label %t12
t12:
  %t12c = icmp eq i32 %tag, 12
  br i1 %t12c, label %n_end, label %t13
t13:
  %t13c = icmp eq i32 %tag, 13
  br i1 %t13c, label %n_iter, label %n_void
n_i128:
  br label %ret_global
n_f64:
  br label %ret_global
n_bool:
  br label %ret_global
n_str:
  br label %ret_global
n_arr:
  br label %ret_global
n_slice:
  br label %ret_global
n_opt:
  br label %ret_global
n_err:
  br label %ret_global
n_ptr:
  br label %ret_global
n_fn:
  br label %ret_global
n_closure:
  br label %ret_global
n_end:
  br label %ret_global
n_iter:
  br label %ret_global
n_void:
  br label %ret_global
ret_global:
  %gp = phi i8* [ @.t_i128, %n_i128 ], [ @.t_f64, %n_f64 ], [ @.t_bool, %n_bool ], [ @.t_str, %n_str ], [ @.t_arr, %n_arr ], [ @.t_slice, %n_slice ], [ @.t_opt, %n_opt ], [ @.t_err, %n_err ], [ @.t_ptr, %n_ptr ], [ @.t_fn, %n_fn ], [ @.t_closure, %n_closure ], [ @.t_end, %n_end ], [ @.t_iter, %n_iter ], [ @.t_void, %n_void ]
  br label %ret_str
ret_str:
  %tp = phi i8* [ %tn, %cls ], [ %en_tn, %en ], [ %gp, %ret_global ]
  %pi = ptrtoint i8* %tp to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %v1 = insertvalue %Value %v0, i128 %pi, 1
  ret %Value %v1
}
"#;

const HC_READ_U64_LE: &str = r#"define %Value @hc_read_u64_le(%Value %v) {
entry:
  %t = extractvalue %Value %v, 0
  %is_str = icmp eq i32 %t, 5
  br i1 %is_str, label %read, label %abort
read:
  %d = extractvalue %Value %v, 1
  %p = trunc i128 %d to i64
  %pp = inttoptr i64 %p to i8*
  %b0 = load i8, i8* %pp
  %u0 = zext i8 %b0 to i64
  %p1 = getelementptr i8, i8* %pp, i64 1
  %b1 = load i8, i8* %p1
  %u1 = zext i8 %b1 to i64
  %s1 = shl i64 %u1, 8
  %o1 = or i64 %u0, %s1
  %p2 = getelementptr i8, i8* %pp, i64 2
  %b2 = load i8, i8* %p2
  %u2 = zext i8 %b2 to i64
  %s2 = shl i64 %u2, 16
  %o2 = or i64 %o1, %s2
  %p3 = getelementptr i8, i8* %pp, i64 3
  %b3 = load i8, i8* %p3
  %u3 = zext i8 %b3 to i64
  %s3 = shl i64 %u3, 24
  %o3 = or i64 %o2, %s3
  %p4 = getelementptr i8, i8* %pp, i64 4
  %b4 = load i8, i8* %p4
  %u4 = zext i8 %b4 to i64
  %s4 = shl i64 %u4, 32
  %o4 = or i64 %o3, %s4
  %p5 = getelementptr i8, i8* %pp, i64 5
  %b5 = load i8, i8* %p5
  %u5 = zext i8 %b5 to i64
  %s5 = shl i64 %u5, 40
  %o5 = or i64 %o4, %s5
  %p6 = getelementptr i8, i8* %pp, i64 6
  %b6 = load i8, i8* %p6
  %u6 = zext i8 %b6 to i64
  %s6 = shl i64 %u6, 48
  %o6 = or i64 %o5, %s6
  %p7 = getelementptr i8, i8* %pp, i64 7
  %b7 = load i8, i8* %p7
  %u7 = zext i8 %b7 to i64
  %s7 = shl i64 %u7, 56
  %o7 = or i64 %o6, %s7
  %z = zext i64 %o7 to i128
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %z, 1
  ret %Value %v1
abort:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

/// fmt_int(i32) String：i128 → 十进制 → 堆缓冲 Str 值（对齐 oracle display）
const HC_FMT_INT: &str = r#"define %Value @hc_fmt_int(%Value %v) {
entry:
  %d = extractvalue %Value %v, 1
  %is_neg = icmp slt i128 %d, 0
  %negv = sub i128 0, %d
  %mag = select i1 %is_neg, i128 %negv, i128 %d
  %buf = alloca [64 x i8], align 1
  br label %loop
loop:
  %val = phi i128 [ %mag, %entry ], [ %quot, %loop ]
  %pos = phi i64 [ 63, %entry ], [ %dec, %loop ]
  %quot = udiv i128 %val, 10
  %rem = urem i128 %val, 10
  %rem64 = trunc i128 %rem to i64
  %ch64 = add i64 %rem64, 48
  %ch = trunc i64 %ch64 to i8
  %dst = getelementptr inbounds [64 x i8], ptr %buf, i64 0, i64 %pos
  store i8 %ch, i8* %dst
  %dec = sub i64 %pos, 1
  %is0 = icmp eq i128 %quot, 0
  br i1 %is0, label %done, label %loop
done:
  %dcount = sub i64 64, %pos
  %extra = select i1 %is_neg, i64 1, i64 0
  %nbytes = add i64 %dcount, %extra
  %allocn = add i64 %nbytes, 1
  %bufh = call i8* @hc_alloc(i64 %allocn)
  %dp = getelementptr inbounds [64 x i8], ptr %buf, i64 0, i64 %pos
  %dstd = getelementptr i8, i8* %bufh, i64 %extra
  call void @llvm.memcpy.p0i8.p0i8.i64(i8* %dstd, i8* %dp, i64 %dcount, i1 false)
  %nuloff = add i64 %dcount, %extra
  %nulp = getelementptr i8, i8* %bufh, i64 %nuloff
  store i8 0, i8* %nulp
  br i1 %is_neg, label %sig, label %mk
sig:
  store i8 45, i8* %bufh
  br label %mk
mk:
  %pi = ptrtoint i8* %bufh to i128
  %x0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %x1 = insertvalue %Value %x0, i128 %pi, 1
  ret %Value %x1
}
"#;

/// fmt_float(f64) String：sprintf 格式（整值 `%.1f`，否则 `%.15g`，对齐 oracle display）；
/// 接受 Int 实参（对齐 interp/IR 的数值提升）
const HC_FMT_FLOAT: &str = r#"define %Value @hc_fmt_float(%Value %v) {
entry:
  %tag = extractvalue %Value %v, 0
  %d = extractvalue %Value %v, 1
  %is_int = icmp eq i32 %tag, 2
  %as_double = sitofp i128 %d to double
  %dt = trunc i128 %d to i64
  %fbits = bitcast i64 %dt to double
  %fv = select i1 %is_int, double %as_double, double %fbits
  %buf = alloca [64 x i8], align 1
  %fr = call double @fmod(double %fv, double 1.0)
  %fz = fcmp oeq double %fr, 0.0
  %fa = call double @fabs(double %fv)
  %fl = fcmp olt double %fa, 1.0e15
  %whole = and i1 %fz, %fl
  br i1 %whole, label %whole_f, label %frac_f
whole_f:
  %p1 = getelementptr inbounds [5 x i8], ptr @.fmt_one, i64 0, i64 0
  call i32 (i8*, ...) @sprintf(i8* %buf, i8* %p1, double %fv)
  br label %mk
frac_f:
  %pg = getelementptr inbounds [6 x i8], ptr @.fmt_g15, i64 0, i64 0
  call i32 (i8*, ...) @sprintf(i8* %buf, i8* %pg, double %fv)
  br label %mk
mk:
  %len = call i64 @strlen(i8* %buf)
  %allocn = add i64 %len, 1
  %bufh = call i8* @hc_alloc(i64 %allocn)
  call void @llvm.memcpy.p0i8.p0i8.i64(i8* %bufh, i8* %buf, i64 %len, i1 false)
  %nulp = getelementptr i8, i8* %bufh, i64 %len
  store i8 0, i8* %nulp
  %pi = ptrtoint i8* %bufh to i128
  %x0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0
  %x1 = insertvalue %Value %x0, i128 %pi, 1
  ret %Value %x1
}
"#;

const HC_ADD_OVERFLOW: &str = r#"define %Value @hc_add_overflow(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %abort
int_op:
  %res = call { i128, i1 } @llvm.sadd.with.overflow.i128(i128 %da, i128 %db)
  %rv = extractvalue { i128, i1 } %res, 0
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %rv, 1
  %arr = call %Value @hc_make_arr(i64 2)
  call void @hc_arr_set(%Value %arr, i64 0, %Value %v1)
  %boolv = call %Value @hc_bool(i1 false)
  call void @hc_arr_set(%Value %arr, i64 1, %Value %boolv)
  ret %Value %arr
abort:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

const HC_SUB_OVERFLOW: &str = r#"define %Value @hc_sub_overflow(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %abort
int_op:
  %res = call { i128, i1 } @llvm.ssub.with.overflow.i128(i128 %da, i128 %db)
  %rv = extractvalue { i128, i1 } %res, 0
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %rv, 1
  %arr = call %Value @hc_make_arr(i64 2)
  call void @hc_arr_set(%Value %arr, i64 0, %Value %v1)
  %boolv = call %Value @hc_bool(i1 false)
  call void @hc_arr_set(%Value %arr, i64 1, %Value %boolv)
  ret %Value %arr
abort:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

const HC_MUL_OVERFLOW: &str = r#"define %Value @hc_mul_overflow(%Value %a, %Value %b) {
entry:
  %ta = extractvalue %Value %a, 0
  %tb = extractvalue %Value %b, 0
  %da = extractvalue %Value %a, 1
  %db = extractvalue %Value %b, 1
  %ai = icmp eq i32 %ta, 2
  %bi = icmp eq i32 %tb, 2
  %both = and i1 %ai, %bi
  br i1 %both, label %int_op, label %abort
int_op:
  %res = call { i128, i1 } @llvm.smul.with.overflow.i128(i128 %da, i128 %db)
  %rv = extractvalue { i128, i1 } %res, 0
  %v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
  %v1 = insertvalue %Value %v0, i128 %rv, 1
  %arr = call %Value @hc_make_arr(i64 2)
  call void @hc_arr_set(%Value %arr, i64 0, %Value %v1)
  %boolv = call %Value @hc_bool(i1 false)
  call void @hc_arr_set(%Value %arr, i64 1, %Value %boolv)
  ret %Value %arr
abort:
  call void @hc_abort_typeerr()
  unreachable
}
"#;

fn emit_scalar_builtin_helpers(out: &mut String) {
    for h in [
        HC_MIN,
        HC_MAX,
        HC_SQRT,
        HC_BOX,
        HC_COPY,
        HC_INTCAST,
        HC_TYPEOF,
        HC_READ_U64_LE,
        HC_FMT_INT,
        HC_FMT_FLOAT,
        HC_ADD_OVERFLOW,
        HC_SUB_OVERFLOW,
        HC_MUL_OVERFLOW,
    ] {
        out.push_str(h);
        out.push('\n');
    }
    // math.* 数值 helper（对齐 oracle call_math）
    out.push_str(&math_unop_helper(
        "hc_abs",
        "%r = call double @fabs(double %f)",
    ));
    out.push_str(&math_unop_helper(
        "hc_floor",
        "%r = call double @floor(double %f)",
    ));
    out.push_str(&math_unop_helper(
        "hc_ceil",
        "%r = call double @ceil(double %f)",
    ));
    out.push_str(&math_unop_helper(
        "hc_round",
        "%r = call double @round(double %f)",
    ));
    out.push_str(&math_unop_helper("hc_pow", "%r = fmul double %f, %f"));
}

// ---------- Phase 7 Io 值构造（main(io: Io) 单参入口 / test_io 绑定） ----------

const HC_MAKE_IO: &str = r#"define %Value @hc_make_io() {
entry:
  %fs = call %Value @hc_make_class(i8* @.t_fs, i64 0)
  %time = call %Value @hc_make_class(i8* @.t_time, i64 0)
  %net = call %Value @hc_make_class(i8* @.t_net, i64 0)
  %io = call %Value @hc_make_class(i8* @.t_io, i64 3)
  %fp = getelementptr inbounds [3 x i8], ptr @.f_fs, i64 0, i64 0
  call void @hc_class_set(%Value %io, i64 0, i8* %fp, %Value %fs)
  %tp = getelementptr inbounds [5 x i8], ptr @.f_time, i64 0, i64 0
  call void @hc_class_set(%Value %io, i64 1, i8* %tp, %Value %time)
  %np = getelementptr inbounds [4 x i8], ptr @.f_net, i64 0, i64 0
  call void @hc_class_set(%Value %io, i64 2, i8* %np, %Value %net)
  ret %Value %io
}
"#;

fn emit_io_helper(out: &mut String) {
    out.push_str(HC_MAKE_IO);
    out.push('\n');
}

// ---------- 断言内建 helper（失败写全局 @hc_fail_msg） ----------

fn emit_assert_helpers(out: &mut String) {
    let amsg = "error.AssertFailed";
    let an = amsg.len() + 1;
    let cases: &[(&str, &str, &str)] = &[
        (
            "hc_expect",
            "%Value %x",
            "%b = call i1 @hc_truthy(%Value %x)",
        ),
        (
            "hc_expect_eq",
            "%Value %x, %Value %y",
            "%b = call i1 @hc_eq(%Value %x, %Value %y)",
        ),
        (
            "hc_expect_neq",
            "%Value %x, %Value %y",
            "%c = call i1 @hc_eq(%Value %x, %Value %y)\n  %b = xor i1 %c, true",
        ),
        (
            "hc_expect_error",
            "%Value %x, %Value %y",
            "%ea = call i1 @hc_is_err(%Value %x)\n  %eb = call i1 @hc_is_err(%Value %y)\n  %eq = call i1 @hc_eq(%Value %x, %Value %y)\n  %e1 = and i1 %ea, %eb\n  %b = and i1 %e1, %eq",
        ),
        (
            "hc_expect_eq_slices",
            "%Value %x, %Value %y",
            "%b = call i1 @hc_eq(%Value %x, %Value %y)",
        ),
    ];
    for (fname, params, cond) in cases {
        let _ = writeln!(out, "define %Value @{fname}({params}) {{");
        let _ = writeln!(out, "entry:");
        for line in cond.lines() {
            let _ = writeln!(out, "  {line}");
        }
        let _ = writeln!(out, "  br i1 %b, label %ok, label %fail");
        let _ = writeln!(out, "ok:");
        let _ = writeln!(out, "  ret %Value {{ i32 0, i128 0 }}");
        let _ = writeln!(out, "fail:");
        let _ = writeln!(
            out,
            "  %fail_msg = getelementptr inbounds [{an} x i8], ptr @.msg_assert, i64 0, i64 0"
        );
        let _ = writeln!(out, "  store i8* %fail_msg, i8** @hc_fail_msg");
        let _ = writeln!(out, "  ret %Value {{ i32 0, i128 0 }}");
        let _ = writeln!(out, "}}\n");
    }
}

// ---------- 函数发射 ----------

/// 槽 → 编译期常量（SSA：每个 `IrInst::Const` 槽位恰好一次）。
/// 供 io.print 格式串 / @sizeOf/@intCast 类型名在 codegen 期解析。
fn build_slot_consts(f: &IrFunc) -> HashMap<usize, IrConst> {
    let mut m = HashMap::new();
    for inst in &f.body {
        if let IrInst::Const { temp, val } = inst {
            m.insert(*temp, val.clone());
        }
    }
    m
}

/// 从槽常量子表读类型名（`@sizeOf(i32)` / `alloc.init(ABC)` 的类型位置参数）。
fn const_str_arg(slot_consts: &HashMap<usize, IrConst>, arg: Option<&usize>) -> Option<String> {
    match arg.and_then(|a| slot_consts.get(a)) {
        Some(IrConst::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

/// @sizeOf(T) 标量表（对齐 run_ir `scalar_size_ir`；用户 class/enum 无布局表 → None）
fn scalar_size_native(ty: &str) -> Option<usize> {
    match ty {
        "i8" | "u8" | "bool" => Some(1),
        "i16" | "u16" | "f16" => Some(2),
        "i32" | "u32" | "f32" => Some(4),
        "i64" | "u64" | "isize" | "usize" | "f64" => Some(8),
        "i128" | "u128" | "f128" => Some(16),
        "String" | "Vec" | "Map" | "Deque" | "Table" | "Allocator" => Some(8),
        _ => None,
    }
}

/// @alignOf(T)（对齐 run_ir：i8/i16/i32/i128 显式，余下 size.min(8)，未知默认 8）
fn align_native(ty: &str) -> usize {
    match ty {
        "i8" | "u8" | "bool" => 1,
        "i16" | "u16" | "f16" => 2,
        "i32" | "u32" | "f32" => 4,
        "i128" | "u128" | "f128" => 16,
        _ => scalar_size_native(ty).map(|s| s.min(8)).unwrap_or(8),
    }
}

/// @intCast 目标宽度范围（对齐 run_ir `int_width_bounds_ir`）
fn int_bounds_native(ty: &str) -> Option<(i128, i128)> {
    match ty {
        "i8" => Some((i8::MIN as i128, i8::MAX as i128)),
        "i16" => Some((i16::MIN as i128, i16::MAX as i128)),
        "i32" => Some((i32::MIN as i128, i32::MAX as i128)),
        "i64" => Some((i64::MIN as i128, i64::MAX as i128)),
        "i128" => Some((i128::MIN, i128::MAX)),
        "isize" => Some((isize::MIN as i128, isize::MAX as i128)),
        "u8" => Some((0, u8::MAX as i128)),
        "u16" => Some((0, u16::MAX as i128)),
        "u32" => Some((0, u32::MAX as i128)),
        "u64" => Some((0, u64::MAX as i128)),
        "u128" => Some((0, u128::MAX as i128)),
        "usize" => Some((0, usize::MAX as i128)),
        _ => None,
    }
}

/// 隐式 Io 实例静态名（`Call{"io.print"}` 形态：root 未解析为局部变量时）
fn is_io_print_name(name: &str) -> bool {
    matches!(
        name,
        "io.print" | "stdout.print" | "stderr.print" | "test_io.print"
    )
}

/// io.print 格式串段（对齐 oracle interp.rs:4042-4079 解析）。
enum PrintSeg {
    Lit(String),
    Arg { slot: Option<usize>, mode: u32 },
}

/// 解析格式串（B1/B3，2026-08-17）：`{}`→显示、`{d}`→十进制、`{x}`→十六进制小写、
/// `{X}`→十六进制大写、`{b}`→二进制、`{e}`→科学计数、`{s}`→显示；宽度/对齐/精度
/// （`{:8}`/`{:<6}`/`{:.2}`）原生暂不填充（值格式化不受影响——原生填充留后续）。
/// 其余字节为字面量。占位符无对应实参（参数不足）→ oracle 跳过（slot=None）。
fn parse_print_fmt(fmt: &str, args: &[usize]) -> Vec<PrintSeg> {
    let bytes = fmt.as_bytes();
    let mut out: Vec<PrintSeg> = Vec::new();
    let mut lit: Vec<u8> = Vec::new();
    let mut argi = 1usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let flush = |lit: &mut Vec<u8>, out: &mut Vec<PrintSeg>| {
            if !lit.is_empty() {
                out.push(PrintSeg::Lit(String::from_utf8_lossy(lit).to_string()));
                lit.clear();
            }
        };
        if bytes[i] == b'{' {
            if let Some(close) = bytes[i + 1..].iter().position(|&c| c == b'}') {
                flush(&mut lit, &mut out);
                // 说明符内：可选 `:` + 对齐/宽度/精度（原生忽略填充）+ 类型字符
                let inner = &bytes[i + 1..i + 1 + close];
                let ty = inner
                    .iter()
                    .rfind(|&&c| {
                        !c.is_ascii_digit()
                            && c != b'.'
                            && c != b':'
                            && c != b'<'
                            && c != b'>'
                            && c != b'^'
                    })
                    .copied();
                let mode = match ty {
                    Some(b'x') => 1,
                    Some(b'b') => 2,
                    Some(b'X') => 4,
                    Some(b'e') => 5,
                    _ => 0, // {} / {d} / {s} / 未识别 → 显示
                };
                out.push(PrintSeg::Arg {
                    slot: args.get(argi).copied(),
                    mode,
                });
                argi += 1;
                i += close + 2;
                continue;
            }
        }
        lit.push(bytes[i]);
        i += 1;
    }
    if !lit.is_empty() {
        out.push(PrintSeg::Lit(String::from_utf8_lossy(&lit).to_string()));
    }
    out
}

fn emit_func(
    out: &mut String,
    f: &IrFunc,
    idx: usize,
    strings: &[String],
    errors: &ErrorCodeTable,
    canon: &HashMap<String, Vec<usize>>,
    funcs: &[IrFunc],
    gidx: &HashMap<String, usize>,
    prefix: &str,
    links: &HashMap<String, String>,
    ext_decls: &mut Vec<(String, usize)>,
) {
    let slot_consts = build_slot_consts(f);
    let _ = writeln!(out, "; {prefix}hc_fn{idx} = {}", f.name);
    let params = (0..f.params.len())
        .map(|i| format!("%Value %p{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "define %Value @\"{prefix}hc_fn{idx}\"({params}) {{");
    // 序言（槽数组 + 参数存槽）并入 entry 块（BodyEmitter 首个块即 entry）
    let mut be = BodyEmitter::new(prefix, links);
    be.emit(format!(
        "%slots = alloca [{} x %Value], align 16",
        f.n_slots
    ));
    for i in 0..f.n_slots {
        be.emit(format!(
            "%sp.{i} = getelementptr inbounds [{n} x %Value], [{n} x %Value]* %slots, i32 0, i32 {i}",
            n = f.n_slots
        ));
    }
    for (i, ps) in f.params.iter().enumerate() {
        be.emit(format!("store %Value %p{i}, %Value* %sp.{ps}"));
    }
    // defer 活跃计数表（Phase 6）：每个 PushDefer id 一个 i32 计数器，entry 置零。
    // 计数 = 本调用内待运行 defers 多重集；PushDefer 增 / PopDefer 减 / 守卫 JumpIfNotDefer 查零。
    // LIFO 顺序由编译期发射顺序保证（计数仅作「是否待运行」判定，无需栈序）。
    let n_defers = f
        .body
        .iter()
        .filter(|i| matches!(i, IrInst::PushDefer { .. }))
        .count();
    if n_defers > 0 {
        be.emit(format!("%defers = alloca [{n_defers} x i32], align 4"));
        for id in 0..n_defers {
            be.emit(format!(
                "%defer.{id} = getelementptr inbounds [{n} x i32], [{n} x i32]* %defers, i32 0, i32 {id}",
                n = n_defers
            ));
            be.emit(format!("store i32 0, i32* %defer.{id}"));
        }
    }
    for inst in &f.body {
        be.inst(inst, strings, errors, canon, funcs, gidx, &slot_consts);
    }
    let per_func = std::mem::take(&mut be.ext_decls);
    out.push_str(&be.finish());
    out.push_str("}\n\n");
    // C3：外部链接符号声明——去重收集到模块级（同一符号被多函数引用时只 declare 一次，
    // LLVM 拒绝重复声明同一函数）；模块末尾统一 emit（LLVM 允许前向引用）。
    for (sym, n) in per_func {
        if !ext_decls.iter().any(|(s, _)| *s == sym) {
            ext_decls.push((sym, n));
        }
    }
}

/// C3：模块级外部链接符号声明（`declare %Value @\"jsonlib.hc_fn0\"(%Value, ...)`——
/// 链接时由库 .a 提供定义）。
fn emit_ext_decls(out: &mut String, ext_decls: &[(String, usize)]) {
    for (sym, n) in ext_decls {
        let params = (0..*n)
            .map(|i| format!("%Value %d{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "declare %Value @\"{sym}\"({params})");
    }
}

// ---------- main 包装（原生 CRT 入口） ----------

/// 发射对全部 `@__init__` 函数的调用（多文件合并 = 各模块 init 依次运行，entry 在前）。
/// `@__init__` 不在 func_index（不可被用户调用），此处按 funcs 声明序找到并执行；
/// 返回值是错误值 → 未处理错误到根（对齐 tree-walking `exec_decl_top` 失败即 panic）。
fn emit_init_calls(out: &mut String, module: &IrModule) {
    for (idx, f) in module.funcs.iter().enumerate() {
        if f.name != "@__init__" {
            continue;
        }
        let _ = writeln!(out, "  %_init{idx} = call %Value @\"hc_fn{idx}\"()");
        let _ = writeln!(out, "  %_tag{idx} = extractvalue %Value %_init{idx}, 0");
        let _ = writeln!(out, "  %_iserr{idx} = icmp eq i32 %_tag{idx}, 6");
        let _ = writeln!(
            out,
            "  br i1 %_iserr{idx}, label %_initerr{idx}, label %_initok{idx}"
        );
        let _ = writeln!(out, "_initerr{idx}:");
        out.push_str("  call void @hc_abort_unhandled()\n  unreachable\n");
        let _ = writeln!(out, "_initok{idx}:");
    }
}

/// 播种隐式环境全局（对齐 IrRuntime::init 的 `implicit_env_value`，ir.rs:4358）：
/// `pi`→Float(PI) 常量、`Vec/Deque/Table`→空 Arr、`io/test_io/stdout/stderr`→Io 实例。
/// 仅在 `@.h_globals` 数组含该名（module.globals 恒含 IMPLICIT_ENV）时发射 store；
/// Map/alloc 需字符串全局（非恒在），未播种——原生路径经名分派（alloc.init 等）不 LoadGlobal。
/// 入口 main/test 均在 `@__init__` 前执行（用户 init 可读隐式环境）。P11d：30-interface 的
/// `Circle.area` 读 `pi` 原生此前为 Void → 0.0；播种后对齐 oracle。
fn emit_implicit_env_seed(out: &mut String, module: &IrModule) {
    let n = module.globals.len();
    let gidx = globals_index(module);
    // pi → Float(PI)
    if let Some(&i) = gidx.get("pi") {
        let bits = std::f64::consts::PI.to_bits() as u128;
        let _ = writeln!(
            out,
            "  %seep = getelementptr inbounds [{n} x %Value], ptr @.h_globals, i64 0, i64 {i}"
        );
        let _ = writeln!(
            out,
            "  store %Value {{ i32 {T_FLOAT}, i128 {bits} }}, %Value* %seep"
        );
    }
    // Vec/Deque/Table → 空 Arr（对齐 implicit_env_value make_arr(空)）
    for name in ["Vec", "Deque", "Table"] {
        if let Some(&i) = gidx.get(name) {
            let _ = writeln!(out, "  %seev{i} = call %Value @hc_make_arr(i64 0)");
            let _ = writeln!(
                out,
                "  %seep{i} = getelementptr inbounds [{n} x %Value], ptr @.h_globals, i64 0, i64 {i}"
            );
            let _ = writeln!(out, "  store %Value %seev{i}, %Value* %seep{i}");
        }
    }
    // io/test_io/stdout/stderr → Io 实例（hc_make_io helper 恒发射）
    for name in ["io", "test_io", "stdout", "stderr"] {
        if let Some(&i) = gidx.get(name) {
            let _ = writeln!(out, "  %seev{i} = call %Value @hc_make_io()");
            let _ = writeln!(
                out,
                "  %seep{i} = getelementptr inbounds [{n} x %Value], ptr @.h_globals, i64 0, i64 {i}"
            );
            let _ = writeln!(out, "  store %Value %seev{i}, %Value* %seep{i}");
        }
    }
}

fn emit_main_wrapper(out: &mut String, module: &IrModule) {
    out.push_str("define i32 @main(i32 %argc, i8** %argv) {\n");
    out.push_str("entry:\n");
    emit_implicit_env_seed(out, module);
    // A3（ADR-0010）：单参 main = args（Vec(String)——argv[0] = 程序名）。
    // 从 argc/argv 构建：hc_make_arr(argc) + 逐元素 Str 值（对齐 run_ir）。
    // 注意：args 循环必须在 entry 块内（init 调用之前）——emit_init_calls 以
    // 分支终结 entry 块，循环放其后会使 phi 前驱错位（%entry 不再是前驱）。
    let mut main_entry: Option<(usize, usize)> = None; // (idx, nparams)
    if let Some(idxs) = module.func_index.get("main") {
        // main 入口按 arity 精确取（无则首个——重载 main 不存在，安全兜底）
        let idx = idxs
            .iter()
            .copied()
            .find(|&i| module.funcs[i].params.is_empty())
            .unwrap_or(idxs[0]);
        let nparams = module.funcs[idx].params.len();
        if nparams == 1 {
            out.push_str("  %argc64 = sext i32 %argc to i64\n");
            out.push_str("  %argvoid = call %Value @hc_make_arr(i64 %argc64)\n");
            out.push_str("  br label %argloop\n");
            out.push_str("argloop:\n");
            out.push_str("  %argi = phi i64 [ 0, %entry ], [ %argnext, %argbody ]\n");
            out.push_str("  %argdone = icmp uge i64 %argi, %argc64\n");
            out.push_str("  br i1 %argdone, label %argdone_l, label %argbody\n");
            out.push_str("argbody:\n");
            out.push_str("  %argp = getelementptr inbounds i8*, i8** %argv, i64 %argi\n");
            out.push_str("  %argstr = load i8*, i8** %argp\n");
            out.push_str("  %argspi = ptrtoint i8* %argstr to i128\n");
            out.push_str("  %argsv0 = insertvalue %Value { i32 0, i128 0 }, i32 5, 0\n");
            out.push_str("  %argsv1 = insertvalue %Value %argsv0, i128 %argspi, 1\n");
            out.push_str("  call void @hc_arr_set(%Value %argvoid, i64 %argi, %Value %argsv1)\n");
            out.push_str("  %argnext = add i64 %argi, 1\n");
            out.push_str("  br label %argloop\n");
            out.push_str("argdone_l:\n");
        } else if nparams > 1 {
            out.push_str("  %argvoid = load %Value, %Value* @.void_value\n");
        }
        main_entry = Some((idx, nparams));
    }
    emit_init_calls(out, module);
    if let Some((idx, nparams)) = main_entry {
        let mut arglist = String::new();
        for _ in 0..nparams {
            if !arglist.is_empty() {
                arglist.push_str(", ");
            }
            arglist.push_str("%Value %argvoid");
        }
        let _ = writeln!(out, "  %r = call %Value @\"hc_fn{idx}\"({arglist})");
        out.push_str("  %tag = extractvalue %Value %r, 0\n");
        out.push_str("  %is_err = icmp eq i32 %tag, 6\n");
        out.push_str("  br i1 %is_err, label %err_exit, label %ok\n");
        out.push_str("err_exit:\n  call void @hc_abort_unhandled()\n  unreachable\n");
        out.push_str("ok:\n  ret i32 0\n");
    } else {
        out.push_str("  ret i32 0\n");
    }
    out.push_str("}\n");
}

/// 原生测试跑器（Q-T5）：按声明序调用每个 `test fn`，全部通过 `ret 0`。
/// 断言失败在测试函数 ret 路径 abort(exit 1)；`return error.X` 由跑器检测 error tag 后
/// abort(exit 1)。因断言失败即 abort，逐测试续跑需重做 assert→返回码通路——故
/// `hc test --mode=compile` 为文件粒度交叉验证（全绿 vs 有失败）。
fn emit_test_runner(out: &mut String, module: &IrModule) {
    let tests: Vec<(usize, &IrFunc)> = module
        .funcs
        .iter()
        .enumerate()
        .filter(|(_, f)| f.is_test)
        .collect();

    // 每个 [test] fn 的运行/通过标记字符串（模块级全局）
    for (idx, f) in &tests {
        let run = format!("[RUN] {}", f.name);
        let pass = format!("[PASS] {}", f.name);
        let _ = writeln!(
            out,
            "@.test.{idx}.run = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
            run.len() + 1,
            llvm_escape(run.as_bytes())
        );
        let _ = writeln!(
            out,
            "@.test.{idx}.pass = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
            pass.len() + 1,
            llvm_escape(pass.as_bytes())
        );
    }

    out.push_str("define i32 @main(i32 %argc, i8** %argv) {\n");
    out.push_str("  %argvoid = load %Value, %Value* @.void_value\n");
    emit_implicit_env_seed(out, module);
    emit_init_calls(out, module);
    for (idx, f) in &tests {
        let run = format!("[RUN] {}", f.name);
        let rn = run.len() + 1;
        let _ = writeln!(
            out,
            "  %runp_{idx} = getelementptr inbounds [{rn} x i8], ptr @.test.{idx}.run, i64 0, i64 0"
        );
        let _ = writeln!(out, "  call i32 @puts(i8* %runp_{idx})");
        let arglist = (0..f.params.len())
            .map(|_| "%Value %argvoid")
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "  %r_{idx} = call %Value @\"hc_fn{idx}\"({arglist})");
        let _ = writeln!(out, "  %tag_{idx} = extractvalue %Value %r_{idx}, 0");
        let _ = writeln!(out, "  %is_err_{idx} = icmp eq i32 %tag_{idx}, 6");
        let _ = writeln!(
            out,
            "  br i1 %is_err_{idx}, label %fail_{idx}, label %ok_{idx}"
        );
        let _ = writeln!(out, "fail_{idx}:");
        out.push_str("  call void @hc_abort_unhandled()\n  unreachable\n");
        let pass = format!("[PASS] {}", f.name);
        let pn = pass.len() + 1;
        let _ = writeln!(out, "ok_{idx}:");
        let _ = writeln!(
            out,
            "  %passp_{idx} = getelementptr inbounds [{pn} x i8], ptr @.test.{idx}.pass, i64 0, i64 0"
        );
        let _ = writeln!(out, "  call i32 @puts(i8* %passp_{idx})");
    }
    out.push_str("  ret i32 0\n}\n");
}

// ---------- 函数体发射（线性 IR → 基本块 CFG） ----------

struct BodyEmitter {
    ssa: usize,
    fresh: usize,
    blocks: Vec<String>,
    cur: String,
    terminated: bool,
    /// C3：函数/内部调用名前缀（库形态 = `{pkg}.`；exe = 空）
    prefix: String,
    /// C3：未登记限定名 → 外部链接符号（exe 链接本地库 .a 用）
    links: HashMap<String, String>,
    /// C3：已引用的外部链接符号 (符号名, 参数个数)——emit_func 末尾补 declare
    ext_decls: Vec<(String, usize)>,
}

impl BodyEmitter {
    fn new(prefix: &str, links: &HashMap<String, String>) -> Self {
        BodyEmitter {
            ssa: 0,
            fresh: 0,
            blocks: Vec::new(),
            cur: "entry:\n".to_string(),
            terminated: false,
            prefix: prefix.to_string(),
            links: links.clone(),
            ext_decls: Vec::new(),
        }
    }

    fn r(&mut self) -> String {
        let n = self.ssa;
        self.ssa += 1;
        format!("%r{n}")
    }

    fn fb(&mut self) -> String {
        let n = self.fresh;
        self.fresh += 1;
        format!("fb{n}")
    }

    fn emit(&mut self, line: String) {
        self.cur.push_str("  ");
        self.cur.push_str(&line);
        self.cur.push('\n');
    }

    fn term(&mut self, line: String) {
        self.emit(line);
        self.terminated = true;
        self.close_block();
    }

    fn close_block(&mut self) {
        if !self.terminated {
            self.cur.push_str("  unreachable\n");
        }
        self.blocks.push(std::mem::take(&mut self.cur));
        self.terminated = false;
    }

    fn label(&mut self, id: usize) {
        if !self.terminated {
            let _ = writeln!(self.cur, "  br label %L{id}");
            self.terminated = true;
        }
        self.close_block();
        self.cur = format!("L{id}:\n");
        self.terminated = false;
    }

    fn cond_br(&mut self, cond: &str, label: usize) {
        let fb = self.fb();
        self.term(format!("br i1 {cond}, label %L{label}, label %{fb}"));
        self.cur = format!("{fb}:\n");
        self.terminated = false;
    }

    /// 特性未支持硬中止：当前块 br 到错误块（hc_abort_{key} + unreachable），
    /// 后续指令落到不可达续块（LLVM 允许；运行即 abort，杜绝静默误编译）。
    fn abort_feature(&mut self, key: &str) {
        let l = self.fb();
        self.term(format!("br label %{l}"));
        self.blocks.push(format!(
            "{l}:\n  call void @hc_abort_{key}()\n  unreachable\n"
        ));
        self.cur = format!("{l}.cont:\n");
        self.terminated = false;
    }

    fn finish(mut self) -> String {
        if !self.cur.is_empty() {
            self.close_block();
        }
        self.blocks.join("")
    }

    fn build_store(&mut self, temp: usize, tag: i32, data: String) {
        let v0 = self.r();
        self.emit(format!(
            "{v0} = insertvalue %Value {{ i32 0, i128 0 }}, i32 {tag}, 0"
        ));
        let v1 = self.r();
        self.emit(format!("{v1} = insertvalue %Value {v0}, i128 {data}, 1"));
        self.emit(format!("store %Value {v1}, %Value* %sp.{temp}"));
    }

    fn const_(&mut self, temp: usize, val: &IrConst, strings: &[String], errors: &ErrorCodeTable) {
        let v = self.const_value(val, strings, errors);
        self.emit(format!("store %Value {v}, %Value* %sp.{temp}"));
    }

    /// 常量 → `%Value` SSA 值（Str 取全局字符串地址；余下 tag+data 直接 insertvalue）
    fn const_value(
        &mut self,
        val: &IrConst,
        strings: &[String],
        errors: &ErrorCodeTable,
    ) -> String {
        if let IrConst::Str(s) = val {
            let idx = strings.iter().position(|x| x == s).unwrap_or(0);
            let n = s.len() + 1;
            let p = self.r();
            self.emit(format!(
                "{p} = getelementptr inbounds [{n} x i8], ptr @.str.{idx}, i64 0, i64 0"
            ));
            let pi = self.r();
            self.emit(format!("{pi} = ptrtoint i8* {p} to i128"));
            // SSA 链：每个 insertvalue 用新名（同寄存器二次定义非法）
            let v0 = self.r();
            self.emit(format!(
                "{v0} = insertvalue %Value {{ i32 0, i128 0 }}, i32 {T_STR}, 0"
            ));
            let v1 = self.r();
            self.emit(format!("{v1} = insertvalue %Value {v0}, i128 {pi}, 1"));
            return v1;
        }
        let (tag, data) = match val {
            IrConst::Int(i) => (T_INT, format!("{i}")),
            IrConst::Float(f) => (T_FLOAT, format!("{}", f.to_bits() as u128)),
            IrConst::Bool(b) => (T_BOOL, if *b { "1" } else { "0" }.to_string()),
            IrConst::Void => (T_VOID, "0".to_string()),
            IrConst::Null => (T_NULL, "0".to_string()),
            IrConst::Err { name, .. } => (T_ERR, format!("{}", errors.code_of(name).unwrap_or(0))),
            IrConst::End => (T_END, "0".to_string()),
            IrConst::Str(_) => unreachable!(),
        };
        let v0 = self.r();
        self.emit(format!(
            "{v0} = insertvalue %Value {{ i32 0, i128 0 }}, i32 {tag}, 0"
        ));
        let v1 = self.r();
        self.emit(format!("{v1} = insertvalue %Value {v0}, i128 {data}, 1"));
        v1
    }

    fn emit_bool_store(&mut self, temp: usize, cond: &str) {
        let b = self.r();
        self.emit(format!("{b} = call %Value @hc_bool(i1 {cond})"));
        self.emit(format!("store %Value {b}, %Value* %sp.{temp}"));
    }

    fn bin(&mut self, op: IrBinOp, temp: usize, a: usize, b: usize) {
        let va = self.r();
        self.emit(format!("{va} = load %Value, %Value* %sp.{a}"));
        let vb = self.r();
        self.emit(format!("{vb} = load %Value, %Value* %sp.{b}"));
        match op {
            IrBinOp::Add
            | IrBinOp::Sub
            | IrBinOp::Mul
            | IrBinOp::Div
            | IrBinOp::Mod
            | IrBinOp::EucMod
            | IrBinOp::BitAnd
            | IrBinOp::BitOr
            | IrBinOp::BitXor
            | IrBinOp::Shl
            | IrBinOp::Shr => {
                let helper = match op {
                    IrBinOp::Add => "hc_add",
                    IrBinOp::Sub => "hc_sub",
                    IrBinOp::Mul => "hc_mul",
                    IrBinOp::Div => "hc_div",
                    IrBinOp::Mod => "hc_mod",
                    IrBinOp::EucMod => "hc_eucmod",
                    IrBinOp::BitAnd => "hc_bitand",
                    IrBinOp::BitOr => "hc_bitor",
                    IrBinOp::BitXor => "hc_bitxor",
                    IrBinOp::Shl => "hc_shl",
                    IrBinOp::Shr => "hc_shr",
                    _ => unreachable!(),
                };
                let res = self.r();
                self.emit(format!(
                    "{res} = call %Value @{helper}(%Value {va}, %Value {vb})"
                ));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrBinOp::Eq | IrBinOp::Ne | IrBinOp::Lt | IrBinOp::Le | IrBinOp::Gt | IrBinOp::Ge => {
                match op {
                    IrBinOp::Eq => {
                        let c = self.r();
                        self.emit(format!("{c} = call i1 @hc_eq(%Value {va}, %Value {vb})"));
                        self.emit_bool_store(temp, &c);
                    }
                    IrBinOp::Ne => {
                        let c = self.r();
                        self.emit(format!("{c} = call i1 @hc_eq(%Value {va}, %Value {vb})"));
                        let n = self.r();
                        self.emit(format!("{n} = xor i1 {c}, true"));
                        self.emit_bool_store(temp, &n);
                    }
                    IrBinOp::Lt => {
                        let c = self.r();
                        self.emit(format!("{c} = call i1 @hc_lt(%Value {va}, %Value {vb})"));
                        self.emit_bool_store(temp, &c);
                    }
                    IrBinOp::Le => {
                        let l = self.r();
                        self.emit(format!("{l} = call i1 @hc_lt(%Value {va}, %Value {vb})"));
                        let e = self.r();
                        self.emit(format!("{e} = call i1 @hc_eq(%Value {va}, %Value {vb})"));
                        let o = self.r();
                        self.emit(format!("{o} = or i1 {l}, {e}"));
                        self.emit_bool_store(temp, &o);
                    }
                    IrBinOp::Gt => {
                        let l = self.r();
                        self.emit(format!("{l} = call i1 @hc_lt(%Value {va}, %Value {vb})"));
                        let e = self.r();
                        self.emit(format!("{e} = call i1 @hc_eq(%Value {va}, %Value {vb})"));
                        let o = self.r();
                        self.emit(format!("{o} = or i1 {l}, {e}"));
                        let g = self.r();
                        self.emit(format!("{g} = xor i1 {o}, true"));
                        self.emit_bool_store(temp, &g);
                    }
                    IrBinOp::Ge => {
                        let l = self.r();
                        self.emit(format!("{l} = call i1 @hc_lt(%Value {va}, %Value {vb})"));
                        let g = self.r();
                        self.emit(format!("{g} = xor i1 {l}, true"));
                        self.emit_bool_store(temp, &g);
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    fn un(&mut self, op: IrUnOp, temp: usize, a: usize) {
        let va = self.r();
        self.emit(format!("{va} = load %Value, %Value* %sp.{a}"));
        let helper = match op {
            IrUnOp::Neg => "hc_neg",
            IrUnOp::Not => "hc_not",
            IrUnOp::BitNot => "hc_bitnot",
        };
        let res = self.r();
        self.emit(format!("{res} = call %Value @{helper}(%Value {va})"));
        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
    }

    /// 从参数槽加载 `%Value` 实参列表（C3 外部链接调用复用；emit load 指令）
    fn arglist(&mut self, args: &[usize]) -> String {
        let mut arglist = String::new();
        for a in args {
            let v = self.r();
            self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
            if !arglist.is_empty() {
                arglist.push_str(", ");
            }
            arglist.push_str(&format!("%Value {v}"));
        }
        arglist
    }

    fn call(
        &mut self,
        name: &str,
        args: &[usize],
        temp: usize,
        canon: &HashMap<String, Vec<usize>>,
        funcs: &[IrFunc],
        strings: &[String],
        errors: &ErrorCodeTable,
        slot_consts: &HashMap<usize, IrConst>,
    ) {
        // Phase 7：隐式环境静态方法形态——root 标识符（io/alloc 等）未解析为局部
        // 变量时，`io.print(...)` / `alloc.init(ABC)` 以点分静态名出现，与 CallMethod
        // 同语义（io 为隐式 Io 实例 / alloc 为隐式 Allocator）。
        if is_io_print_name(name) {
            self.call_print(args, temp, slot_consts, strings);
            return;
        }
        if name == "alloc.init" {
            self.call_alloc_init(args, temp, slot_consts, strings);
            return;
        }
        // math.nan/inf/inf_neg/sqrt/abs/pow/floor/ceil/round（对齐 oracle call_math）
        if let Some(field) = name.strip_prefix("math.") {
            self.call_math(field, args, temp);
            return;
        }
        let Some(candidates) = canon.get(name) else {
            // C3：未登记限定名 → 外部链接符号（exe 链接本地库 .a：`jsonlib.parse` →
            // `@{pkg}.hc_fn{i}`）；未命中才 Phase 7 响亮拒绝（禁止 NoFunction 静默歧义）
            if name.contains('.') {
                if let Some(sym) = self.links.get(name).cloned() {
                    if !self.ext_decls.iter().any(|(s, _)| *s == sym) {
                        self.ext_decls.push((sym.clone(), args.len()));
                    }
                    let res = self.r();
                    let arglist = self.arglist(args);
                    self.emit(format!("{res} = call %Value @\"{sym}\"({arglist})"));
                    self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
                    return;
                }
                self.abort_feature("builtin");
                return;
            }
            let res = self.r();
            self.emit(format!("{res} = call %Value @hc_no_function()"));
            self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            return;
        };
        // 重载静态分派（对齐 `pick_func` ①③）：先精确参数数，无则全池（尾参默认回退）
        let exact: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|&i| funcs[i].params.len() == args.len())
            .collect();
        let pool: Vec<usize> = if exact.is_empty() {
            candidates.clone()
        } else {
            exact
        };
        let target = pool[0]; // 同 arity 多候选 → 首个（类型分派留待 Phase 7）
        let fdef = &funcs[target];
        let mut arglist = self.arglist(args);
        // 尾参默认值（编译期常量）补齐
        for d in fdef.defaults.iter().skip(args.len()) {
            if let Some(c) = d {
                let v = self.const_value(c, strings, errors);
                if !arglist.is_empty() {
                    arglist.push_str(", ");
                }
                arglist.push_str(&format!("%Value {v}"));
            }
        }
        let res = self.r();
        self.emit(format!(
            "{res} = call %Value @\"{}hc_fn{target}\"({arglist})",
            self.prefix
        ));
        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
    }

    /// math.* 原生 codegen（对齐 oracle call_math interp.rs:4922-4960）：
    /// nan/inf/inf_neg 忽略类型名参数，直接发 Float 位模式常量；数值函数经一元
    /// helper（Int→f64 强制 / Float 直用）返回 Float。
    fn call_math(&mut self, field: &str, args: &[usize], temp: usize) {
        match field {
            "nan" => self.build_store(temp, T_FLOAT, format!("{}", f64::NAN.to_bits() as u128)),
            "inf" => self.build_store(
                temp,
                T_FLOAT,
                format!("{}", f64::INFINITY.to_bits() as u128),
            ),
            "inf_neg" => self.build_store(
                temp,
                T_FLOAT,
                format!("{}", f64::NEG_INFINITY.to_bits() as u128),
            ),
            "sqrt" => self.emit_unop_helper("hc_sqrt", args, temp),
            "abs" => self.emit_unop_helper("hc_abs", args, temp),
            "floor" => self.emit_unop_helper("hc_floor", args, temp),
            "ceil" => self.emit_unop_helper("hc_ceil", args, temp),
            "round" => self.emit_unop_helper("hc_round", args, temp),
            "pow" => self.emit_unop_helper("hc_pow", args, temp),
            _ => self.abort_feature("builtin"),
        }
    }

    fn call_builtin(
        &mut self,
        name: &str,
        args: &[usize],
        temp: usize,
        slot_consts: &HashMap<usize, IrConst>,
        _strings: &[String],
        _errors: &ErrorCodeTable,
    ) {
        // ---------- 断言族（现有 5 helper） ----------
        if let Some(helper) = match name {
            "expect" => Some("hc_expect"),
            "expect_eq" => Some("hc_expect_eq"),
            "expect_neq" => Some("hc_expect_neq"),
            "expect_error" => Some("hc_expect_error"),
            "expect_eq_slices" => Some("hc_expect_eq_slices"),
            _ => None,
        } {
            let mut arglist = String::new();
            for a in args {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
                if !arglist.is_empty() {
                    arglist.push_str(", ");
                }
                arglist.push_str(&format!("%Value {v}"));
            }
            let res = self.r();
            self.emit(format!("{res} = call %Value @{helper}({arglist})"));
            self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            return;
        }
        // ---------- @ 内建（类型位置参数在 slot_consts 以 Const Str 存在） ----------
        if name == "@sizeOf" || name == "@alignOf" {
            let Some(ty) = const_str_arg(slot_consts, args.first()) else {
                self.abort_feature("builtin");
                return;
            };
            let v = if name == "@sizeOf" {
                match scalar_size_native(&ty) {
                    Some(s) => s as i128,
                    None => {
                        self.abort_feature("builtin");
                        return;
                    }
                }
            } else {
                align_native(&ty) as i128
            };
            self.build_store(temp, T_INT, v.to_string());
            return;
        }
        if name == "@intCast" {
            let Some(ty) = const_str_arg(slot_consts, args.first()) else {
                self.abort_feature("builtin");
                return;
            };
            let Some((min, max)) = int_bounds_native(&ty) else {
                self.abort_feature("builtin");
                return;
            };
            let Some(&vslot) = args.get(1) else {
                self.abort_feature("builtin");
                return;
            };
            let v = self.r();
            self.emit(format!("{v} = load %Value, %Value* %sp.{vslot}"));
            let res = self.r();
            self.emit(format!(
                "{res} = call %Value @hc_intcast(%Value {v}, i128 {min}, i128 {max})"
            ));
            self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            return;
        }
        if name == "@typeOf" {
            let Some(&vslot) = args.first() else {
                self.abort_feature("builtin");
                return;
            };
            let v = self.r();
            self.emit(format!("{v} = load %Value, %Value* %sp.{vslot}"));
            let res = self.r();
            self.emit(format!("{res} = call %Value @hc_typeof(%Value {v})"));
            self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            return;
        }
        if name == "@ptrCast" || name == "@alignCast" {
            // tag1 指针无类型化——透传（对齐 run_ir：取末参原样返回）
            let Some(&vslot) = args.last() else {
                self.abort_feature("builtin");
                return;
            };
            let v = self.r();
            self.emit(format!("{v} = load %Value, %Value* %sp.{vslot}"));
            self.emit(format!("store %Value {v}, %Value* %sp.{temp}"));
            return;
        }
        if name == "@addWithOverflow" || name == "@subWithOverflow" || name == "@mulWithOverflow" {
            let helper = match name {
                "@addWithOverflow" => "hc_add_overflow",
                "@subWithOverflow" => "hc_sub_overflow",
                _ => "hc_mul_overflow",
            };
            self.emit_binop_helper(helper, args, temp);
            return;
        }
        // ---------- 数值/字节内建 ----------
        match name {
            "min" => {
                self.emit_binop_helper("hc_min", args, temp);
                return;
            }
            "max" => {
                self.emit_binop_helper("hc_max", args, temp);
                return;
            }
            "sqrt" => {
                self.emit_unop_helper("hc_sqrt", args, temp);
                return;
            }
            "box" => {
                self.emit_unop_helper("hc_box", args, temp);
                return;
            }
            "copy" => {
                if args.is_empty() {
                    self.abort_feature("builtin");
                    return;
                }
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{}", args[0]));
                // 模式参（.shallow/.deep）为运行期 Enum——传给 hc_copy 运行时分派；
                // 无模式参 → 传 Void（落入 deep 路径，对齐 run_ir 默认深拷贝）
                let mode = if args.len() > 1 {
                    let m = self.r();
                    self.emit(format!("{m} = load %Value, %Value* %sp.{}", args[1]));
                    m
                } else {
                    self.r();
                    "%Value { i32 0, i128 0 }".to_string()
                };
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_copy(%Value {v}, {mode})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
                return;
            }
            "read_u64_le" => {
                self.emit_unop_helper("hc_read_u64_le", args, temp);
                return;
            }
            "fmt_int" => {
                self.emit_unop_helper("hc_fmt_int", args, temp);
                return;
            }
            "fmt_float" => {
                self.emit_unop_helper("hc_fmt_float", args, temp);
                return;
            }
            _ => {}
        }
        // ---------- 其余内建（sort/binary_search/集合/json/csv/io/fs/时间）→ 响亮拒绝 ----------
        self.abort_feature("builtin");
    }

    /// 单参 helper（sqrt/box/read_u64_le）：加载首参 → 调用 → 存槽。
    fn emit_unop_helper(&mut self, helper: &str, args: &[usize], temp: usize) {
        let Some(&vslot) = args.first() else {
            self.abort_feature("builtin");
            return;
        };
        let v = self.r();
        self.emit(format!("{v} = load %Value, %Value* %sp.{vslot}"));
        let res = self.r();
        self.emit(format!("{res} = call %Value @{helper}(%Value {v})"));
        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
    }

    /// 双参 helper（min/max/溢出）：加载两参 → 调用 → 存槽。
    fn emit_binop_helper(&mut self, helper: &str, args: &[usize], temp: usize) {
        if args.len() < 2 {
            self.abort_feature("builtin");
            return;
        }
        let va = self.r();
        self.emit(format!("{va} = load %Value, %Value* %sp.{}", args[0]));
        let vb = self.r();
        self.emit(format!("{vb} = load %Value, %Value* %sp.{}", args[1]));
        let res = self.r();
        self.emit(format!(
            "{res} = call %Value @{helper}(%Value {va}, %Value {vb})"
        ));
        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
    }

    /// alloc.init(T)：P11d 起已知 class 类型名在 lowering 已降级为默认字段 MakeClass
    /// （`lower_alloc_init_defaults`），实参即类实例 → 原样返回（对齐 run_ir
    /// `call_alloc_method_ir` "init" 的 `IrValue::Class` 分支）。
    /// 仅非类名类型（Const Str 仍在 slot_consts）落入旧路径：无字段空实例（Phase 7 子集）。
    fn call_alloc_init(
        &mut self,
        args: &[usize],
        temp: usize,
        slot_consts: &HashMap<usize, IrConst>,
        strings: &[String],
    ) {
        if let Some(ty) = const_str_arg(slot_consts, args.first()) {
            // 类型名字符串（非类名，如内建类型）→ 空实例（旧路径）
            let (ti, tn) = str_idx(strings, &ty);
            let g = self.r();
            self.emit(format!(
                "{g} = getelementptr inbounds [{tn} x i8], ptr @.str.{ti}, i64 0, i64 0"
            ));
            let res = self.r();
            self.emit(format!(
                "{res} = call %Value @hc_make_class(i8* {g}, i64 0)"
            ));
            self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            return;
        }
        // 实参已是类实例（lower_alloc_init_defaults 的 MakeClass 结果）→ 原样返回
        let Some(&vslot) = args.first() else {
            self.abort_feature("builtin");
            return;
        };
        let v = self.r();
        self.emit(format!("{v} = load %Value, %Value* %sp.{vslot}"));
        self.emit(format!("store %Value {v}, %Value* %sp.{temp}"));
    }

    /// `io.print(fmt, args...)`（含静态形态 `Call{"io.print"}` 与实例 `CallMethod`）。
    /// 格式串必须为编译期字面量（slot_consts 命中）——动态格式串响亮拒绝（禁止静默误编译）。
    fn call_print(
        &mut self,
        args: &[usize],
        temp: usize,
        slot_consts: &HashMap<usize, IrConst>,
        strings: &[String],
    ) {
        let Some(fmt) = args
            .first()
            .and_then(|a| slot_consts.get(a))
            .and_then(|c| match c {
                IrConst::Str(s) => Some(s.clone()),
                _ => None,
            })
        else {
            self.abort_feature("builtin");
            return;
        };
        let segs = parse_print_fmt(&fmt, args);
        for seg in &segs {
            match seg {
                PrintSeg::Lit(s) => {
                    let (si, sn) = str_idx(strings, s);
                    let g = self.r();
                    self.emit(format!(
                        "{g} = getelementptr inbounds [{sn} x i8], ptr @.str.{si}, i64 0, i64 0"
                    ));
                    self.emit(format!(
                        "call void @hc_write_bytes(i8* {g}, i64 {})",
                        s.len()
                    ));
                }
                PrintSeg::Arg {
                    slot: Some(slot),
                    mode,
                } => {
                    let v = self.r();
                    self.emit(format!("{v} = load %Value, %Value* %sp.{slot}"));
                    self.emit(format!("call void @hc_write_value(%Value {v}, i32 {mode})"));
                }
                // 占位符无对应实参 → oracle 跳过该占位符（interp.rs call_io_print）
                PrintSeg::Arg { slot: None, .. } => {}
            }
        }
        // io.print 返回 Void
        self.emit(format!(
            "store %Value {{ i32 0, i128 0 }}, %Value* %sp.{temp}"
        ));
    }

    /// 实例方法分派：解引用基值 → 类名（%ClassObj 首字段）→ 链式 strcmp 匹配拥有者
    /// `{Type}.{method}`（canon）。内建 Io.print 优先；匹配到用户方法则调用 `hc_fn{k}`
    /// 且 self（解引用后值）注入首参（对齐 run_ir 的 self_v = deref_value(base)）。
    fn call_method(
        &mut self,
        temp: usize,
        base: usize,
        method: &str,
        args: &[usize],
        canon: &HashMap<String, Vec<usize>>,
        funcs: &[IrFunc],
        strings: &[String],
        errors: &ErrorCodeTable,
        slot_consts: &HashMap<usize, IrConst>,
    ) {
        // 内建集合方法（Arr 接收者，无 canon 拥有者）：append/append_u64/extend/init/len 等。
        // 对齐 run_ir `call_builtin_method` 的 Arr 臂——运行时按 tag 分派，非 Arr 落入 Class 用户
        // 方法分派（io.print / `{Type}.{method}`）。
        let is_coll_method = matches!(
            method,
            "append" | "push_back" | "append_u64" | "extend" | "init" | "len"
        );

        // 编译期候选拥有者：内建 Io.print + 用户 `{Type}.{method}`（canon 键点分前缀）
        let mut owners: Vec<String> = Vec::new();
        if method == "print" {
            owners.push("Io".to_string());
        }
        let mut user_owners: Vec<String> = canon
            .keys()
            .filter_map(|k| k.strip_suffix(&format!(".{method}")).map(|p| p.to_string()))
            .collect();
        user_owners.sort();
        user_owners.dedup();
        owners.extend(user_owners);
        if owners.is_empty() && !is_coll_method {
            self.abort_feature("nomethod");
            return;
        }

        // 运行时基址：load base → deref → tag
        let bv = self.r();
        self.emit(format!("{bv} = load %Value, %Value* %sp.{base}"));
        let dv = self.r();
        self.emit(format!("{dv} = call %Value @hc_deref(%Value {bv})"));
        let tag = self.r();
        self.emit(format!("{tag} = extractvalue %Value {dv}, 0"));
        let is_cls = self.r();
        self.emit(format!("{is_cls} = icmp eq i32 {tag}, {T_CLASS}"));
        let is_arr = self.r();
        self.emit(format!("{is_arr} = icmp eq i32 {tag}, {T_ARR}"));
        let l_done = self.fb();

        if is_coll_method {
            // Arr 接收者 → 内建集合方法；否则落 Class 分派
            let l_coll = self.fb();
            let l_cls = self.fb();
            self.term(format!("br i1 {is_arr}, label %{l_coll}, label %{l_cls}"));
            self.cur = format!("{l_coll}:\n");
            self.terminated = false;
            self.call_coll_method(temp, method, &dv, args);
            self.term(format!("br label %{l_done}"));
            self.cur = format!("{l_cls}:\n");
            self.terminated = false;
        }

        // Class 分派：is_cls → disp（取类名链式 strcmp）/ notcls → abort
        let l_notcls = self.fb();
        let l_disp = self.fb();
        self.term(format!(
            "br i1 {is_cls}, label %{l_disp}, label %{l_notcls}"
        ));
        self.blocks.push(format!(
            "{l_notcls}:\n  call void @hc_abort_nomethod()\n  unreachable\n"
        ));
        self.cur = format!("{l_disp}:\n");
        self.terminated = false;
        let d1 = self.r();
        self.emit(format!("{d1} = extractvalue %Value {dv}, 1"));
        let op = self.r();
        self.emit(format!("{op} = inttoptr i128 {d1} to %ClassObj*"));
        let co = self.r();
        self.emit(format!("{co} = load %ClassObj, %ClassObj* {op}"));
        let cname = self.r();
        self.emit(format!("{cname} = extractvalue %ClassObj {co}, 0"));

        // 链式 strcmp：每个拥有者一个比较块；命中 → found 处理器；全不中 → abort
        for owner in &owners {
            let (oi, on) = str_idx(strings, owner);
            let og = self.r();
            self.emit(format!(
                "{og} = getelementptr inbounds [{on} x i8], ptr @.str.{oi}, i64 0, i64 0"
            ));
            let cmp = self.r();
            self.emit(format!("{cmp} = call i32 @strcmp(i8* {cname}, i8* {og})"));
            let eq = self.r();
            self.emit(format!("{eq} = icmp eq i32 {cmp}, 0"));
            let l_found = self.fb();
            let l_next = self.fb();
            self.term(format!("br i1 {eq}, label %{l_found}, label %{l_next}"));

            // found 块
            self.cur = format!("{l_found}:\n");
            self.terminated = false;
            if owner == "Io" && method == "print" {
                self.call_print(args, temp, slot_consts, strings);
            } else {
                self.call_method_user(
                    owner, method, &dv, args, temp, canon, funcs, strings, errors,
                );
            }
            self.term(format!("br label %{l_done}"));

            // 下一比较块
            self.cur = format!("{l_next}:\n");
            self.terminated = false;
        }
        // 全部不中 → 硬中止；续块换成 done（后续指令落入 done）
        self.abort_feature("nomethod");
        self.cur = format!("{l_done}:\n");
        self.terminated = false;
    }

    /// Arr 接收者内建集合方法（对齐 run_ir `call_builtin_method` Arr 臂 ir.rs:6085-6306）。
    /// `dv` 为解引用后的基值（tag 8，data = `%ArrObj*`）。变更方法（append/append_u64/
    /// extend）原地改写 `%ArrObj`——所有别名共享同一堆指针，写入即对接收者槽/字段可见。
    fn call_coll_method(&mut self, temp: usize, method: &str, dv: &str, args: &[usize]) {
        let load_arg = |s: &mut Self, slot: usize| {
            let v = s.r();
            s.emit(format!("{v} = load %Value, %Value* %sp.{slot}"));
            v
        };
        match method {
            "init" => {
                // Vec(u8).init(alloc)：返回空 Arr（alloc 实参忽略）
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_make_arr(i64 0)"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            "len" => {
                let d = self.r();
                self.emit(format!("{d} = extractvalue %Value {dv}, 1"));
                let op = self.r();
                self.emit(format!("{op} = inttoptr i128 {d} to %ArrObj*"));
                let ao = self.r();
                self.emit(format!("{ao} = load %ArrObj, %ArrObj* {op}"));
                let al = self.r();
                self.emit(format!("{al} = extractvalue %ArrObj {ao}, 0"));
                let ai = self.r();
                self.emit(format!("{ai} = zext i64 {al} to i128"));
                let v0 = self.r();
                self.emit(format!(
                    "{v0} = insertvalue %Value {{ i32 0, i128 0 }}, i32 {T_INT}, 0"
                ));
                let v1 = self.r();
                self.emit(format!("{v1} = insertvalue %Value {v0}, i128 {ai}, 1"));
                self.emit(format!("store %Value {v1}, %Value* %sp.{temp}"));
            }
            "append" | "push_back" => {
                let Some(&a0) = args.first() else {
                    self.abort_feature("nomethod");
                    return;
                };
                let v = load_arg(self, a0);
                self.emit(format!("call void @hc_append(%Value {dv}, %Value {v})"));
                self.emit(format!(
                    "store %Value {{ i32 0, i128 0 }}, %Value* %sp.{temp}"
                ));
            }
            "append_u64" => {
                let Some(&a0) = args.first() else {
                    self.abort_feature("nomethod");
                    return;
                };
                let v = load_arg(self, a0);
                self.emit(format!("call void @hc_append_u64(%Value {dv}, %Value {v})"));
                self.emit(format!(
                    "store %Value {{ i32 0, i128 0 }}, %Value* %sp.{temp}"
                ));
            }
            "extend" => {
                let Some(&a0) = args.first() else {
                    self.abort_feature("nomethod");
                    return;
                };
                let v = load_arg(self, a0);
                self.emit(format!("call void @hc_extend(%Value {dv}, %Value {v})"));
                self.emit(format!(
                    "store %Value {{ i32 0, i128 0 }}, %Value* %sp.{temp}"
                ));
            }
            _ => {
                self.abort_feature("nomethod");
            }
        }
    }

    /// 用户方法静态调用：canon `{Type}.{method}` 按 arity（含 self）精确分派，
    /// 调 `hc_fn{k}` 且解引用后基值注入首参（对齐 run_ir self_v = deref_value(base)）。
    fn call_method_user(
        &mut self,
        owner: &str,
        method: &str,
        dv: &str,
        args: &[usize],
        temp: usize,
        canon: &HashMap<String, Vec<usize>>,
        funcs: &[IrFunc],
        strings: &[String],
        errors: &ErrorCodeTable,
    ) {
        let key = format!("{owner}.{method}");
        let candidates = canon.get(&key).cloned().unwrap_or_default();
        let exact: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|&i| funcs[i].params.len() == args.len() + 1)
            .collect();
        let pool = if exact.is_empty() {
            candidates.clone()
        } else {
            exact
        };
        let Some(&target) = pool.first() else {
            self.abort_feature("nomethod");
            return;
        };
        let fdef = &funcs[target];
        let mut arglist = format!("%Value {dv}");
        for a in args {
            let v = self.r();
            self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
            arglist.push_str(&format!(", %Value {v}"));
        }
        for d in fdef.defaults.iter().skip(args.len() + 1) {
            if let Some(c) = d {
                let v = self.const_value(c, strings, errors);
                arglist.push_str(&format!(", %Value {v}"));
            }
        }
        let res = self.r();
        self.emit(format!(
            "{res} = call %Value @\"{}hc_fn{target}\"({arglist})",
            self.prefix
        ));
        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
    }

    fn ret(&mut self, slot: usize) {
        let f = self.r();
        self.emit(format!("{f} = load i8*, i8** @hc_fail_msg"));
        let has = self.r();
        self.emit(format!("{has} = icmp ne i8* {f}, null"));
        let fb_fail = self.fb();
        let fb_ok = self.fb();
        self.term(format!("br i1 {has}, label %{fb_fail}, label %{fb_ok}"));
        self.blocks.push(format!(
            "{fb_fail}:\n  call void @hc_abort(i8* {f})\n  unreachable\n"
        ));
        self.cur = format!("{fb_ok}:\n");
        self.terminated = false;
        let v = self.r();
        self.emit(format!("{v} = load %Value, %Value* %sp.{slot}"));
        self.term(format!("ret %Value {v}"));
    }

    fn ret_void(&mut self) {
        let f = self.r();
        self.emit(format!("{f} = load i8*, i8** @hc_fail_msg"));
        let has = self.r();
        self.emit(format!("{has} = icmp ne i8* {f}, null"));
        let fb_fail = self.fb();
        let fb_ok = self.fb();
        self.term(format!("br i1 {has}, label %{fb_fail}, label %{fb_ok}"));
        self.blocks.push(format!(
            "{fb_fail}:\n  call void @hc_abort(i8* {f})\n  unreachable\n"
        ));
        self.cur = format!("{fb_ok}:\n");
        self.terminated = false;
        self.term("ret %Value { i32 0, i128 0 }".to_string());
    }

    fn inst(
        &mut self,
        inst: &IrInst,
        strings: &[String],
        errors: &ErrorCodeTable,
        canon: &HashMap<String, Vec<usize>>,
        funcs: &[IrFunc],
        gidx: &HashMap<String, usize>,
        slot_consts: &HashMap<usize, IrConst>,
    ) {
        match inst {
            IrInst::Const { temp, val } => self.const_(*temp, val, strings, errors),
            IrInst::Load { temp, slot } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{slot}"));
                self.emit(format!("store %Value {v}, %Value* %sp.{temp}"));
            }
            IrInst::Store { slot, temp } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                self.emit(format!("store %Value {v}, %Value* %sp.{slot}"));
            }
            // P11d [continuous] 值语义：运行时门（值实际类名 ∈ module.continuous）→
            // `hc_deep_copy` 递归拷贝；否则恒等（标量/数组/非连续类 = 引用别名）
            IrInst::DeepCopy { temp, a } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
                let c = self.r();
                self.emit(format!("{c} = call %Value @hc_deep_copy_cont(%Value {v})"));
                self.emit(format!("store %Value {c}, %Value* %sp.{temp}"));
            }
            IrInst::Bin { op, temp, a, b } => self.bin(*op, *temp, *a, *b),
            IrInst::Un { op, temp, a } => self.un(*op, *temp, *a),
            IrInst::Jump { label } => {
                self.term(format!("br label %L{label}"));
                let fb = self.fb();
                self.cur = format!("{fb}:\n");
                self.terminated = false;
            }
            IrInst::JumpIf { temp, label } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                let c = self.r();
                self.emit(format!("{c} = call i1 @hc_truthy(%Value {v})"));
                self.cond_br(&c, *label);
            }
            IrInst::JumpIfNot { temp, label } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                let c = self.r();
                self.emit(format!("{c} = call i1 @hc_truthy(%Value {v})"));
                let n = self.r();
                self.emit(format!("{n} = xor i1 {c}, true"));
                self.cond_br(&n, *label);
            }
            IrInst::JumpIfNull { temp, label } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                let c = self.r();
                self.emit(format!("{c} = call i1 @hc_is_null(%Value {v})"));
                self.cond_br(&c, *label);
            }
            IrInst::JumpIfErr { temp, label } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                let c = self.r();
                self.emit(format!("{c} = call i1 @hc_is_err(%Value {v})"));
                self.cond_br(&c, *label);
            }
            IrInst::Label { id } => self.label(*id),
            // Phase 1 指针：取址 = 槽地址入 tag 7 载荷；解引用/写穿经运行时 helper。
            // AddrValue 与 AddrSlot 同一 codegen——快照值已在临时槽中，地址即该槽。
            IrInst::AddrSlot { temp, slot } => {
                let p = self.r();
                self.emit(format!("{p} = ptrtoint %Value* %sp.{slot} to i128"));
                self.build_store(*temp, T_PTR, p);
            }
            IrInst::AddrValue { temp, value } => {
                let p = self.r();
                self.emit(format!("{p} = ptrtoint %Value* %sp.{value} to i128"));
                self.build_store(*temp, T_PTR, p);
            }
            IrInst::Deref { temp, a } => {
                let va = self.r();
                self.emit(format!("{va} = load %Value, %Value* %sp.{a}"));
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_deref(%Value {va})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::StorePtr { target, value } => {
                let vt = self.r();
                self.emit(format!("{vt} = load %Value, %Value* %sp.{target}"));
                let vv = self.r();
                self.emit(format!("{vv} = load %Value, %Value* %sp.{value}"));
                self.emit(format!("call void @hc_store_ptr(%Value {vt}, %Value {vv})"));
            }
            // ---- Phase 2 聚合 ----
            IrInst::Field { temp, base, field } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let (si, sn) = str_idx(strings, field);
                let fg = self.r();
                self.emit(format!(
                    "{fg} = getelementptr inbounds [{sn} x i8], ptr @.str.{si}, i64 0, i64 0"
                ));
                let res = self.r();
                self.emit(format!(
                    "{res} = call %Value @hc_field(%Value {b}, i8* {fg})"
                ));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::StoreField { base, field, value } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{value}"));
                let (si, sn) = str_idx(strings, field);
                let fg = self.r();
                self.emit(format!(
                    "{fg} = getelementptr inbounds [{sn} x i8], ptr @.str.{si}, i64 0, i64 0"
                ));
                self.emit(format!(
                    "call void @hc_store_field(%Value {b}, i8* {fg}, %Value {v})"
                ));
            }
            IrInst::Index { temp, base, index } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let i = self.r();
                self.emit(format!("{i} = load %Value, %Value* %sp.{index}"));
                let res = self.r();
                self.emit(format!(
                    "{res} = call %Value @hc_index(%Value {b}, %Value {i})"
                ));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::StoreIndex { base, index, value } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let i = self.r();
                self.emit(format!("{i} = load %Value, %Value* %sp.{index}"));
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{value}"));
                self.emit(format!(
                    "call void @hc_store_index(%Value {b}, %Value {i}, %Value {v})"
                ));
            }
            IrInst::SliceOf { temp, base, lo, hi } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let lo_v = self.r();
                self.emit(format!("{lo_v} = load %Value, %Value* %sp.{lo}"));
                let hi_v = self.r();
                self.emit(format!("{hi_v} = load %Value, %Value* %sp.{hi}"));
                let res = self.r();
                self.emit(format!(
                    "{res} = call %Value @hc_slice(%Value {b}, %Value {lo_v}, %Value {hi_v})"
                ));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::StoreSlice {
                base,
                lo,
                hi,
                value,
            } => {
                let b = self.r();
                self.emit(format!("{b} = load %Value, %Value* %sp.{base}"));
                let lo_v = self.r();
                self.emit(format!("{lo_v} = load %Value, %Value* %sp.{lo}"));
                let hi_v = self.r();
                self.emit(format!("{hi_v} = load %Value, %Value* %sp.{hi}"));
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{value}"));
                self.emit(format!("call void @hc_store_slice(%Value {b}, %Value {lo_v}, %Value {hi_v}, %Value {v})"));
            }
            IrInst::MakeArr { temp, items } => {
                let arr = self.r();
                self.emit(format!(
                    "{arr} = call %Value @hc_make_arr(i64 {})",
                    items.len()
                ));
                for (i, it) in items.iter().enumerate() {
                    let v = self.r();
                    self.emit(format!("{v} = load %Value, %Value* %sp.{it}"));
                    self.emit(format!(
                        "call void @hc_arr_set(%Value {arr}, i64 {i}, %Value {v})"
                    ));
                }
                self.emit(format!("store %Value {arr}, %Value* %sp.{temp}"));
            }
            IrInst::MakeClass { temp, ty, fields } => {
                let (ti, tn) = str_idx(strings, ty);
                let tyg = self.r();
                self.emit(format!(
                    "{tyg} = getelementptr inbounds [{tn} x i8], ptr @.str.{ti}, i64 0, i64 0"
                ));
                let cls = self.r();
                self.emit(format!(
                    "{cls} = call %Value @hc_make_class(i8* {tyg}, i64 {})",
                    fields.len()
                ));
                for (i, (fname, vslot)) in fields.iter().enumerate() {
                    let v = self.r();
                    self.emit(format!("{v} = load %Value, %Value* %sp.{vslot}"));
                    let (fi, flen) = str_idx(strings, fname);
                    let fg = self.r();
                    self.emit(format!(
                        "{fg} = getelementptr inbounds [{flen} x i8], ptr @.str.{fi}, i64 0, i64 0"
                    ));
                    self.emit(format!(
                        "call void @hc_class_set(%Value {cls}, i64 {i}, i8* {fg}, %Value {v})"
                    ));
                }
                self.emit(format!("store %Value {cls}, %Value* %sp.{temp}"));
            }
            IrInst::MakeEnum {
                temp,
                name,
                variant,
                payload,
            } => {
                let (ni, nn) = str_idx(strings, name);
                let ng = self.r();
                self.emit(format!(
                    "{ng} = getelementptr inbounds [{nn} x i8], ptr @.str.{ni}, i64 0, i64 0"
                ));
                let (vi, vn) = str_idx(strings, variant);
                let vg = self.r();
                self.emit(format!(
                    "{vg} = getelementptr inbounds [{vn} x i8], ptr @.str.{vi}, i64 0, i64 0"
                ));
                match payload {
                    Some(pslot) => {
                        let pv = self.r();
                        self.emit(format!("{pv} = load %Value, %Value* %sp.{pslot}"));
                        let sz = self.r();
                        self.emit(format!("{sz} = ptrtoint %Value* getelementptr (%Value, %Value* null, i32 1) to i64"));
                        let raw = self.r();
                        self.emit(format!("{raw} = call i8* @hc_alloc(i64 {sz})"));
                        let cell = self.r();
                        self.emit(format!("{cell} = bitcast i8* {raw} to %Value*"));
                        self.emit(format!("store %Value {pv}, %Value* {cell}"));
                        let res = self.r();
                        self.emit(format!(
                            "{res} = call %Value @hc_make_enum(i8* {ng}, i8* {vg}, %Value* {cell})"
                        ));
                        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
                    }
                    None => {
                        let res = self.r();
                        self.emit(format!(
                            "{res} = call %Value @hc_make_enum(i8* {ng}, i8* {vg}, %Value* null)"
                        ));
                        self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
                    }
                }
            }
            IrInst::Destructure { value, slots } => {
                let n = slots.len() as i64;
                let sv = self.r();
                self.emit(format!("{sv} = load %Value, %Value* %sp.{value}"));
                let dv = self.r();
                self.emit(format!("{dv} = call %Value @hc_deref(%Value {sv})"));
                let st = self.r();
                self.emit(format!("{st} = extractvalue %Value {dv}, 0"));
                let ia = self.r();
                self.emit(format!("{ia} = icmp eq i32 {st}, 8"));
                let l_arr = self.fb();
                let l_tup = self.fb();
                self.term(format!("br i1 {ia}, label %{l_arr}, label %{l_tup}"));
                self.blocks.push(format!(
                    "{l_tup}:\n  call void @hc_abort_tuplearity()\n  unreachable\n"
                ));
                self.cur = format!("{l_arr}:\n");
                self.terminated = false;
                let sd = self.r();
                self.emit(format!("{sd} = extractvalue %Value {dv}, 1"));
                let op = self.r();
                self.emit(format!("{op} = inttoptr i128 {sd} to %ArrObj*"));
                let ao = self.r();
                self.emit(format!("{ao} = load %ArrObj, %ArrObj* {op}"));
                let alen = self.r();
                self.emit(format!("{alen} = extractvalue %ArrObj {ao}, 0"));
                let items = self.r();
                self.emit(format!("{items} = extractvalue %ArrObj {ao}, 1"));
                let le = self.r();
                self.emit(format!("{le} = icmp eq i64 {alen}, {n}"));
                let l_slots = self.fb();
                let l_arity = self.fb();
                self.term(format!("br i1 {le}, label %{l_slots}, label %{l_arity}"));
                self.blocks.push(format!(
                    "{l_arity}:\n  call void @hc_abort_tuplearity()\n  unreachable\n"
                ));
                self.cur = format!("{l_slots}:\n");
                self.terminated = false;
                for (i, s) in slots.iter().enumerate() {
                    if let Some(slot) = s {
                        let p = self.r();
                        self.emit(format!(
                            "{p} = getelementptr %Value, %Value* {items}, i64 {i}"
                        ));
                        let ev = self.r();
                        self.emit(format!("{ev} = load %Value, %Value* {p}"));
                        self.emit(format!("store %Value {ev}, %Value* %sp.{slot}"));
                    }
                }
            }
            IrInst::Move { temp, a } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
                self.emit(format!("store %Value {v}, %Value* %sp.{temp}"));
            }
            IrInst::Unwrap { temp, a } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{a}"));
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_unwrap(%Value {v})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            // ---- Phase 3 switch / 区间 / for ----
            IrInst::MatchTest {
                temp,
                subject,
                pattern,
            } => {
                let sv = self.r();
                self.emit(format!("{sv} = load %Value, %Value* %sp.{subject}"));
                // 模式描述符：0=Error(code) 1=Ident(str) 2=Int(data) 3=Float(bits) 4=Str(str,len) 5=Char(data)
                let (ptag, pdata, pstr, plen) = match pattern {
                    IrPattern::Error(name) => {
                        let code = errors.code_of(name).unwrap_or(0);
                        (0u8, code as u128, "null".to_string(), 0i64)
                    }
                    IrPattern::Ident(s) => {
                        let (si, sn) = str_idx(strings, s);
                        let pg = self.r();
                        self.emit(format!("{pg} = getelementptr inbounds [{sn} x i8], ptr @.str.{si}, i64 0, i64 0"));
                        (1u8, 0u128, pg, 0i64)
                    }
                    IrPattern::Int(i) => (2u8, *i as u128, "null".to_string(), 0i64),
                    IrPattern::Float(f) => (3u8, f.to_bits() as u128, "null".to_string(), 0i64),
                    IrPattern::Str(s) => {
                        let (si, sn) = str_idx(strings, s);
                        let pg = self.r();
                        self.emit(format!("{pg} = getelementptr inbounds [{sn} x i8], ptr @.str.{si}, i64 0, i64 0"));
                        (4u8, 0u128, pg, s.len() as i64)
                    }
                    IrPattern::Char(c) => (5u8, *c as u128, "null".to_string(), 0i64),
                };
                let res = self.r();
                self.emit(format!(
                    "{res} = call %Value @hc_match_test(%Value {sv}, i8 {ptag}, i128 {pdata}, i8* {pstr}, i64 {plen})"
                ));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::MakeRange { temp, lo, hi } => {
                let lv = self.r();
                self.emit(format!("{lv} = load %Value, %Value* %sp.{lo}"));
                let hv = self.r();
                self.emit(format!("{hv} = load %Value, %Value* %sp.{hi}"));
                let res = self.r();
                self.emit(format!(
                    "{res} = call %Value @hc_make_range(%Value {lv}, %Value {hv})"
                ));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::EnumPayload { temp, a } => {
                let av = self.r();
                self.emit(format!("{av} = load %Value, %Value* %sp.{a}"));
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_enum_payload(%Value {av})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::IterMake { temp, base } => {
                let bv = self.r();
                self.emit(format!("{bv} = load %Value, %Value* %sp.{base}"));
                let res = self.r();
                self.emit(format!("{res} = call %Value @hc_iter_make(%Value {bv})"));
                self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
            }
            IrInst::IterNext {
                has,
                iter,
                slot,
                read_only,
            } => {
                // 捕获槽为按值 `%Value`：先拷贝项值入槽，循环体末尾由 IterWriteBack 写回。
                // `read_only` 不影响 LLVM 布局（只读捕获槽无人写；Mut/Move 靠写回收敛）。
                let _ = read_only;
                let iv = self.r();
                self.emit(format!("{iv} = load %Value, %Value* %sp.{iter}"));
                let ip = self.r();
                self.emit(format!("{ip} = extractvalue %Value {iv}, 1"));
                let itp = self.r();
                self.emit(format!("{itp} = inttoptr i128 {ip} to %IterObj*"));
                let res = self.r();
                self.emit(format!(
                    "{res} = call %Value @hc_iter_next(%IterObj* {itp}, %Value* %sp.{slot})"
                ));
                self.emit(format!("store %Value {res}, %Value* %sp.{has}"));
            }
            IrInst::IterWriteBack { iter, slot } => {
                let iv = self.r();
                self.emit(format!("{iv} = load %Value, %Value* %sp.{iter}"));
                let ip = self.r();
                self.emit(format!("{ip} = extractvalue %Value {iv}, 1"));
                let itp = self.r();
                self.emit(format!("{itp} = inttoptr i128 {ip} to %IterObj*"));
                self.emit(format!(
                    "call void @hc_iter_write_back(%IterObj* {itp}, %Value* %sp.{slot})"
                ));
            }
            IrInst::Call { name, args, temp } => {
                self.call(
                    name,
                    args,
                    *temp,
                    canon,
                    funcs,
                    strings,
                    errors,
                    slot_consts,
                );
            }
            IrInst::CallBuiltin { name, args, temp } => {
                self.call_builtin(name, args, *temp, slot_consts, strings, errors)
            }
            IrInst::CallMethod {
                temp,
                base,
                method,
                args,
            } => self.call_method(
                *temp,
                *base,
                method,
                args,
                canon,
                funcs,
                strings,
                errors,
                slot_consts,
            ),
            // Phase 4 原生后端临时取舍：闭包/函数引用/间接调用需原生 ABI 改造（Phase 8），
            // 当前响亮拒绝（error.NotCallable），禁止静默误编译。
            // 方法调用已由 Phase 7 内建/用户类分派覆盖；闭包仍在 Phase 8。
            IrInst::MakeClosure { .. } => self.abort_feature("notcallable"),
            IrInst::FnRef { .. } => self.abort_feature("notcallable"),
            IrInst::CallIndirect { .. } => self.abort_feature("notcallable"),
            IrInst::Return { temp } => self.ret(*temp),
            IrInst::ReturnVoid => self.ret_void(),
            // Phase 5 全局单元：`@.h_globals` 数组寻址读写（声明序槽位）
            IrInst::LoadGlobal { temp, name } => {
                // 可变隐式容器（G4，0acd0e5）：每次加载**合成新空容器**——对齐 run_ir
                // `implicit_env_value`（每次新建）。共享全局槽 → append 重分配后全部
                // Vec 字段别名同一容器 → 自引用递归段错误（31-class/46-recursion 回归）。
                if matches!(name.as_str(), "Vec" | "Deque" | "Table") {
                    let gv = self.r();
                    self.emit(format!("{gv} = call %Value @hc_make_arr(i64 0)"));
                    self.emit(format!("store %Value {gv}, %Value* %sp.{temp}"));
                    return;
                }
                if let Some(&i) = gidx.get(name) {
                    let gp = self.r();
                    self.emit(format!(
                        "{gp} = getelementptr inbounds [{n} x %Value], ptr @.h_globals, i64 0, i64 {i}",
                        n = gidx.len()
                    ));
                    let gv = self.r();
                    self.emit(format!("{gv} = load %Value, %Value* {gp}"));
                    self.emit(format!("store %Value {gv}, %Value* %sp.{temp}"));
                } else {
                    self.abort_feature("noglobal");
                }
            }
            IrInst::StoreGlobal { name, value } => {
                if let Some(&i) = gidx.get(name) {
                    let gv = self.r();
                    self.emit(format!("{gv} = load %Value, %Value* %sp.{value}"));
                    let gp = self.r();
                    self.emit(format!(
                        "{gp} = getelementptr inbounds [{n} x %Value], ptr @.h_globals, i64 0, i64 {i}",
                        n = gidx.len()
                    ));
                    self.emit(format!("store %Value {gv}, %Value* {gp}"));
                } else {
                    self.abort_feature("noglobal");
                }
            }
            // `&global`：全局单元数组元素地址入 tag 7（Ptr）载荷——与局部 AddrSlot
            // 同构，Deref/StorePtr 写穿经 `@hc_deref`/`@hc_store_ptr` 回全局。
            IrInst::GlobalAddr { temp, name } => {
                if let Some(&i) = gidx.get(name) {
                    let gp = self.r();
                    self.emit(format!(
                        "{gp} = getelementptr inbounds [{n} x %Value], ptr @.h_globals, i64 0, i64 {i}",
                        n = gidx.len()
                    ));
                    let p = self.r();
                    self.emit(format!("{p} = ptrtoint %Value* {gp} to i128"));
                    self.build_store(*temp, T_PTR, p);
                } else {
                    self.abort_feature("noglobal");
                }
            }
            // ---- Phase 6：defer / errdefer ----
            IrInst::PushDefer { id } => {
                let c = self.r();
                self.emit(format!("{c} = load i32, i32* %defer.{id}"));
                let c1 = self.r();
                self.emit(format!("{c1} = add i32 {c}, 1"));
                self.emit(format!("store i32 {c1}, i32* %defer.{id}"));
            }
            IrInst::PopDefer { id } => {
                let c = self.r();
                self.emit(format!("{c} = load i32, i32* %defer.{id}"));
                let c1 = self.r();
                self.emit(format!("{c1} = sub i32 {c}, 1"));
                self.emit(format!("store i32 {c1}, i32* %defer.{id}"));
            }
            IrInst::JumpIfNotDefer { id, label } => {
                let c = self.r();
                self.emit(format!("{c} = load i32, i32* %defer.{id}"));
                let z = self.r();
                self.emit(format!("{z} = icmp eq i32 {c}, 0"));
                self.cond_br(&z, *label);
            }
        }
    }
}

// ---------- 纯文本发射测试（不依赖 zig/clang） ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir;

    fn gen(src: &str) -> String {
        let p = crate::parse_source(src).expect("parse");
        let m = ir::lower(&p).expect("lower");
        let t = crate::errorcodes::collect(&p, 0);
        codegen(&m, &t)
    }

    #[test]
    fn scalar_main_defines_and_wrapper() {
        let ll = gen("fn main() i32 { return 42; }");
        assert!(ll.contains("define %Value @\"hc_fn0\"()"), "{ll}");
        assert!(
            ll.contains("define i32 @main(i32 %argc, i8** %argv)"),
            "{ll}"
        );
        assert!(ll.contains("i128 42"), "{ll}");
        assert!(ll.contains("ret %Value"), "{ll}");
    }

    #[test]
    fn add_emits_overflow_intrinsic() {
        let ll = gen("fn add(a: i32, b: i32) i32 { return a + b; }");
        assert!(ll.contains("@llvm.sadd.with.overflow.i128"), "{ll}");
        assert!(ll.contains("define %Value @hc_add"), "{ll}");
    }

    #[test]
    fn err_literal_uses_error_code() {
        let ll = gen("fn f() !i32 { return error.NotFound; }");
        assert!(ll.contains("i32 6"), "{ll}"); // err tag
                                               // error.NotFound 码 = 0（包 ID 0 + 首个错误）
        assert!(
            ll.contains("insertvalue %Value { i32 0, i128 0 }, i32 6, 0"),
            "{ll}"
        );
    }

    #[test]
    fn string_literal_emits_global() {
        let ll = gen("fn f() String { return \"hi\"; }");
        assert!(ll.contains("@.str.0 = private"), "{ll}");
        assert!(ll.contains("c\"hi\\00\""), "{ll}");
        assert!(ll.contains("ptrtoint i8*"), "{ll}");
    }

    #[test]
    fn if_while_emit_labels_and_branches() {
        let ll = gen(
            "fn sum(n: i32) i32 { var mut i: i32 = 0; var mut s: i32 = 0; while (i < n) : (i += 1) { s += i; } return s; }",
        );
        assert!(ll.contains("br label %L"), "{ll}");
        assert!(ll.contains("br i1"), "{ll}");
        assert!(ll.contains("call i1 @hc_truthy"), "{ll}");
    }

    #[test]
    fn try_catch_uses_err_channel() {
        let ll = gen("fn f() !i32 { return error.NotFound; } fn g() i32 { return f() catch 7; }");
        assert!(ll.contains("call i1 @hc_is_err"), "{ll}");
    }

    #[test]
    fn pointer_addr_of_deref_store_emit_helpers() {
        // `&mut x` → AddrSlot（槽地址入 tag 7）；`p.*` → Deref；`p.* = 7` → StorePtr
        let ll = gen("fn f() i32 { var mut x: i32 = 5; var p = &mut x; p.* = 7; return x; }");
        assert!(ll.contains("define %Value @hc_deref"), "{ll}");
        assert!(ll.contains("define void @hc_store_ptr"), "{ll}");
        assert!(ll.contains("ptrtoint %Value* %sp."), "{ll}");
        assert!(ll.contains("call %Value @hc_deref"), "{ll}");
        assert!(ll.contains("call void @hc_store_ptr"), "{ll}");
        // 写穿返回结果应等于 7（常量 7 入载荷）
        assert!(ll.contains("i128 7"), "{ll}");
    }

    #[test]
    fn pointer_bad_assign_has_message() {
        let ll = gen("fn f() void { return; }");
        assert!(ll.contains("@.msg_badassign"), "{ll}");
        assert!(ll.contains("error.BadAssign"), "{ll}");
    }

    #[test]
    fn pointer_eq_uses_identity_dispatch() {
        // 同目标指针相等 → hc_eq 分流到纯值比较（先 hc_deref 归一化）
        let ll = gen(
            "fn f() bool { var mut x: i32 = 5; var p = &mut x; var q = &mut x; return p == q; }",
        );
        assert!(ll.contains("define i1 @hc_eq("), "{ll}");
        assert!(ll.contains("define i1 @hc_eq_plain("), "{ll}");
        assert!(ll.contains("call i1 @hc_eq_plain"), "{ll}");
        assert!(ll.contains("call %Value @hc_deref"), "{ll}");
    }

    #[test]
    fn pointer_write_through_deref_value() {
        // 复合赋值 `p.* += 1`：Deref + Bin + StorePtr 三序列齐
        let ll = gen("fn f() i32 { var mut x: i32 = 5; var p = &mut x; p.* += 1; return p.*; }");
        assert!(ll.contains("call %Value @hc_deref"), "{ll}");
        assert!(ll.contains("call %Value @hc_add"), "{ll}");
        assert!(ll.contains("call void @hc_store_ptr"), "{ll}");
    }

    // ---- Phase 2 聚合发射 ----

    #[test]
    fn aggregate_field_emits_class_helpers() {
        // MakeClass/Field/StoreField → hc_make_class/hc_class_set/hc_field/hc_store_field
        let ll = gen(
            r#"class P { x: i32, y: i32, } fn f() i32 { var p = P{ x = 1, y = 2 }; p.y = 5; return p.x; }"#,
        );
        assert!(ll.contains("define %Value @hc_make_class"), "{ll}");
        assert!(ll.contains("define void @hc_class_set"), "{ll}");
        assert!(ll.contains("define %Value @hc_field"), "{ll}");
        assert!(ll.contains("define void @hc_store_field"), "{ll}");
        assert!(ll.contains("call %Value @hc_make_class"), "{ll}");
        assert!(ll.contains("call void @hc_class_set"), "{ll}");
        assert!(ll.contains("call %Value @hc_field"), "{ll}");
        assert!(ll.contains("call void @hc_store_field"), "{ll}");
        // 字符串全局（类型名 + 字段名）被收集
        assert!(ll.contains("@.str.0 = private"), "{ll}");
    }

    #[test]
    fn aggregate_array_index_emits_arr_helpers() {
        // MakeArr/Index/StoreIndex → hc_make_arr/hc_arr_set/hc_index/hc_store_index
        let ll = gen("fn f() i32 { var a = [10, 20, 30]; a[1] = 99; return a[1]; }");
        assert!(ll.contains("define %Value @hc_make_arr"), "{ll}");
        assert!(ll.contains("define void @hc_arr_set"), "{ll}");
        assert!(ll.contains("define %Value @hc_index"), "{ll}");
        assert!(ll.contains("define void @hc_store_index"), "{ll}");
        assert!(ll.contains("call %Value @hc_make_arr"), "{ll}");
        assert!(ll.contains("call void @hc_arr_set"), "{ll}");
        assert!(ll.contains("call %Value @hc_index"), "{ll}");
        assert!(ll.contains("call void @hc_store_index"), "{ll}");
    }

    #[test]
    fn aggregate_append_emits_coll_helpers() {
        // `Vec.append`（Arr 接收者内建方法）→ hc_append 定义 + 调用；append_u64/extend
        // helper 随 preamble 发射（供 57-protocol 等）。对齐 run_ir `call_builtin_method`
        // Arr 臂的原地扩容语义（同一 `%ArrObj` 指针写回，别名可见）。
        let ll = gen(
            r#"class Node { value: i32, children: Vec(Node), } fn f() i32 {
    var r = Node.new(1, alloc);
    r.children.append(Node.new(2, alloc));
    r.children.append(Node.new(3, alloc));
    return r.children.len;
}"#,
        );
        assert!(ll.contains("define void @hc_append"), "{ll}");
        assert!(ll.contains("define void @hc_append_u64"), "{ll}");
        assert!(ll.contains("define void @hc_extend"), "{ll}");
        assert!(ll.contains("call void @hc_append"), "{ll}");
    }

    #[test]
    fn aggregate_slice_emits_slice_helpers() {
        // SliceOf/StoreSlice → hc_slice/hc_store_slice + %SliceObj
        let ll = gen(
            "fn f() i32 { var a = [1, 2, 3, 4, 5]; var s = a[1..3]; a[1..3] = [8, 9]; return s[0]; }",
        );
        assert!(ll.contains("define %Value @hc_slice"), "{ll}");
        assert!(ll.contains("define void @hc_store_slice"), "{ll}");
        assert!(ll.contains("%SliceObj = type"), "{ll}");
        assert!(ll.contains("call %Value @hc_slice"), "{ll}");
        assert!(ll.contains("call void @hc_store_slice"), "{ll}");
    }

    #[test]
    fn aggregate_enum_emits_make_enum() {
        // MakeEnum → hc_make_enum（name+variant 字符串 GEP）
        let ll = gen("enum Color { red, green, blue } fn f() Color { return Color.green; }");
        assert!(ll.contains("define %Value @hc_make_enum"), "{ll}");
        assert!(ll.contains("call %Value @hc_make_enum"), "{ll}");
    }

    #[test]
    fn aggregate_unwrap_emits_hc_unwrap() {
        let ll = gen("fn boxed(x: ?i32) ?i32 { return x; } fn f() i32 { return boxed(7).?; }");
        assert!(ll.contains("define %Value @hc_unwrap"), "{ll}");
        assert!(ll.contains("call %Value @hc_unwrap"), "{ll}");
        assert!(ll.contains("@.msg_nullunwrap"), "{ll}");
    }

    #[test]
    fn aggregate_deep_eq_emits_hc_eq_agg() {
        // 类/数组/枚举深比较 → hc_eq_agg（hc_eq 分流）
        let ll = gen("fn f() bool { var a = [1, 2, 3]; var b = [1, 2, 3]; return a == b; }");
        assert!(ll.contains("define i1 @hc_eq_agg"), "{ll}");
        assert!(ll.contains("call i1 @hc_eq_agg"), "{ll}");
        assert!(ll.contains("define %SeqInfo @hc_seq_info"), "{ll}");
    }

    #[test]
    fn aggregate_tuple_destructure_emits_split() {
        // Destructure：返回 `(i32, i32)` → `hc_make_arr` + 按索引取值
        let ll = gen(
            "fn d(a: i32, b: i32) (i32, i32) { return (a, b); } fn f() i32 { var (q, r) = d(3, 4); return q + r; }",
        );
        assert!(ll.contains("call %Value @hc_make_arr"), "{ll}");
    }

    #[test]
    fn no_inline_constant_expr_gep() {
        // LLVM 18+ 移除 `getelementptr` 常量表达式：所有 GEP 必须发射为
        // SSA 指令（`%rN = getelementptr ...` 或 sizeof 惯用法 `ptrtoint ...`），
        // 不得作为 call/select 操作数内联（`i8* getelementptr ...`）。
        let ll = gen(
            r#"class P { x: i32, } fn boxed(x: ?i32) ?i32 { return x; } fn f() i32 {
            var p = P{ x = 1 };
            var s = "abc";
            var a = [1, 2, 3];
            var v = boxed(7).?;
            if (p.x == s.len and a[0] == v) { return 1; }
            return 0; }"#,
        );
        for line in ll.lines() {
            let t = line.trim_start();
            // 指令形式（`%rN = ...`）与全局声明不含此特征；行内常量表达式
            // GEP 必然以 `i8* getelementptr` / `i8* (getelementptr` 形态出现。
            if t.contains("i8* getelementptr") || t.contains("i8* (getelementptr") {
                panic!("行内常量表达式 GEP 残留: {line}");
            }
        }
    }

    // ---- Phase 3 switch + range + for 发射 ----

    #[test]
    fn phase3_switch_emits_match_test_helper() {
        // MatchTest 指令 → @hc_match_test（模式描述符：tag/data/str/len）
        let ll = gen("fn f(x: i32) i32 { switch (x) { 1 => return 10, else => return 99, } }");
        assert!(ll.contains("define %Value @hc_match_test"), "{ll}");
        assert!(ll.contains("call %Value @hc_match_test"), "{ll}");
        // switch 指令：i8 类型限定的 case 列表
        assert!(ll.contains("switch i8 %tag, label %done"), "{ll}");
        assert!(ll.contains("i8 0, label %t_err"), "{ll}");
        assert!(ll.contains("i8 2, label %t_int"), "{ll}");
    }

    #[test]
    fn phase3_switch_string_pattern_collected() {
        // Str/Ident 模式字符串进入字符串表（@.str.N 全局）
        let ll =
            gen(r#"fn pick(s: String) i32 { switch (s) { "hi" => return 1, else => return 0, } }"#);
        assert!(ll.contains("@hc_match_test"), "{ll}");
        assert!(ll.contains("c\"hi\\00\""), "{ll}");
    }

    #[test]
    fn phase3_enum_payload_emits_helper() {
        // EnumPayload 指令 → @hc_enum_payload
        let ll = gen(
            r#"enum Maybe { some: i32, none } fn f(m: Maybe) i32 { switch (m) { some => |i| i, none => -1, } }"#,
        );
        assert!(ll.contains("define %Value @hc_enum_payload"), "{ll}");
        assert!(ll.contains("call %Value @hc_enum_payload"), "{ll}");
    }

    #[test]
    fn phase3_make_range_emits_helper() {
        // MakeRange 指令 → @hc_make_range（区间 [lo, hi) → Arr）
        let ll = gen("fn f() i32 { var mut s: i32 = 0; for (0..4) |i| { s += i; } return s; }");
        assert!(ll.contains("define %Value @hc_make_range"), "{ll}");
        assert!(ll.contains("call %Value @hc_make_range"), "{ll}");
    }

    #[test]
    fn phase3_iter_emits_iter_helpers() {
        // IterMake/IterNext/IterWriteBack → @hc_iter_make/@hc_iter_next/@hc_iter_write_back
        let ll = gen("fn f() i32 { var a = [1, 2]; for (a) |mut x| { x += 1; } return a[1]; }");
        assert!(ll.contains("define %IterObj* @hc_iter_alloc"), "{ll}");
        assert!(ll.contains("define void @hc_iter_set"), "{ll}");
        assert!(ll.contains("define %Value @hc_iter_make"), "{ll}");
        assert!(ll.contains("define %Value @hc_iter_next"), "{ll}");
        assert!(ll.contains("define void @hc_iter_write_back"), "{ll}");
        assert!(ll.contains("call %Value @hc_iter_make"), "{ll}");
        assert!(ll.contains("call %Value @hc_iter_next"), "{ll}");
        assert!(ll.contains("call void @hc_iter_write_back"), "{ll}");
        assert!(ll.contains("%IterItemObj = type"), "{ll}");
        assert!(ll.contains("%IterObj = type"), "{ll}");
    }

    #[test]
    fn phase3_iter_notiter_message_present() {
        // NotIterable 硬错误消息：@.msg_notiter 全局 + hc_abort_notiter helper
        let ll = gen(
            "fn f() i32 { var a = [1]; var mut s: i32 = 0; for (a) |x| { s += x; } return s; }",
        );
        assert!(ll.contains("@.msg_notiter"), "{ll}");
        assert!(ll.contains("define void @hc_abort_notiter"), "{ll}");
    }

    // ---- Phase 7 原生内建 helper（子集） ----

    #[test]
    fn phase7_io_print_emits_write_helpers() {
        // main(io: Io) 的 io.print：单参 main 注入 @hc_make_io()；格式串切分为
        // 字面量段（hc_write_bytes）+ 参数槽（hc_write_value，模式 0=显示）。
        let ll = gen("fn main(io: Io) !void { io.print(\"x = {}, y = {}\\n\", 42, 3.14); }");
        assert!(ll.contains("call %Value @hc_make_io()"), "{ll}");
        assert!(ll.contains("define %Value @hc_make_io()"), "{ll}");
        assert!(ll.contains("define void @hc_write_bytes"), "{ll}");
        assert!(ll.contains("define void @hc_write_value"), "{ll}");
        // 字面量段 "x = " 与 "\n" 登记为独立全局（不是格式串整体）
        assert!(ll.contains("c\"x = \\00\""), "{ll}");
        assert!(ll.contains("c\"\\0A\\00\""), "{ll}");
        assert!(ll.contains("call void @hc_write_bytes"), "{ll}");
        assert!(ll.contains("call void @hc_write_value"), "{ll}");
    }

    #[test]
    fn phase7_alloc_init_emits_make_class() {
        // alloc.init(ABC) → MakeClass 默认字段 → @hc_make_class(ABC 类型名全局, i64 1)（1 字段）
        let ll =
            gen("class ABC { x: i32, } fn main() i32 { var abc = alloc.init(ABC); return abc.x; }");
        assert!(ll.contains("call %Value @hc_make_class(i8*"), "{ll}");
        assert!(ll.contains("i64 1)"), "{ll}");
        assert!(ll.contains("c\"ABC\\00\""), "{ll}");
    }

    #[test]
    fn phase7_math_builtins_emit_nan_and_helpers() {
        // math.nan(f64) 类型名参数 → 忽略，直接发 NaN 位模式 Float；
        // math.sqrt/abs/pow → 一元 helper 调用；helper 定义进 preamble。
        let ll = gen(r#"fn main(io: Io) !void {
    var nan = math.nan(f64);
    var s = math.sqrt(4.0);
    var p = math.pow(3.0);
    io.print("{} {} {}\n", nan, s, p);
}"#);
        // f64::NAN.to_bits() = 0x7FF8000000000000
        assert!(ll.contains("i32 3, 0"), "{ll}");
        assert!(ll.contains("i128 9221120237041090560"), "{ll}");
        assert!(ll.contains("call %Value @hc_sqrt(%Value"), "{ll}");
        assert!(ll.contains("call %Value @hc_pow(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_abs(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_floor(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_ceil(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_round(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_pow(%Value"), "{ll}");
    }

    #[test]
    fn phase7_user_method_dispatch_emits_strcmp_chain() {
        // abc.print(&io)：运行时分派——解引用基值 → 类名 strcmp 匹配 "ABC" →
        // 调 hc_fn{k} 且 self（解引用后值）注入首参。
        let ll = gen(r#"class ABC {
    pub fn print(self: *Self, io: *Io) { io.print("m\n"); }
}
fn main(io: Io) !void {
    var abc = alloc.init(ABC);
    abc.print(&io);
}"#);
        assert!(ll.contains("call i32 @strcmp"), "{ll}");
        assert!(ll.contains("c\"ABC\\00\""), "{ll}");
        assert!(ll.contains("call %Value @hc_deref"), "{ll}");
        assert!(ll.contains("hc_fn"), "{ll}");
        // self 注入：方法分派后至少一处以解引用值调 hc_fn{k}
        assert!(ll.contains("call %Value @\"hc_fn"), "{ll}");
    }

    #[test]
    fn phase7_scalar_builtins_emit_helpers() {
        // 标量 @ 内建 + 自由内建 → 各自 helper 调用（不再静默 Void）
        let ll = gen(r#"fn main() !void {
    try expect_eq(@sizeOf(i32), 4);
    try expect_eq(@intCast(i32, 7), 7);
    try expect_eq(@typeOf(42), "i128");
    try expect_eq(min(3, 9), 3);
    try expect_eq(max(3, 9), 9);
    var p = box(42);
    try expect_eq(p.*, 42);
}"#);
        assert!(ll.contains("call %Value @hc_intcast"), "{ll}");
        assert!(ll.contains("call %Value @hc_typeof"), "{ll}");
        assert!(ll.contains("call %Value @hc_min"), "{ll}");
        assert!(ll.contains("call %Value @hc_max"), "{ll}");
        assert!(ll.contains("call %Value @hc_box"), "{ll}");
        // @sizeOf 编译期常量折叠 → i32 槽直接存 4（非 helper 调用）
        assert!(ll.contains("store %Value"), "{ll}");
    }

    #[test]
    fn phase7_fmt_builtins_emit_str_helpers() {
        // D3：fmt_int/fmt_float 自由内建 → hc_fmt_int/hc_fmt_float helper 调用；
        // helper 定义进 preamble（hc_alloc 堆缓冲 Str 值）。
        let ll = gen(r#"fn main() !void {
    var s1 = fmt_int(30);
    var s2 = fmt_float(3.5);
    io.print("{} {}\n", s1, s2);
}"#);
        assert!(ll.contains("call %Value @hc_fmt_int(%Value"), "{ll}");
        assert!(ll.contains("call %Value @hc_fmt_float(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_fmt_int(%Value"), "{ll}");
        assert!(ll.contains("define %Value @hc_fmt_float(%Value"), "{ll}");
        // sprintf 声明（hc_fmt_float 数字→字符串用）
        assert!(ll.contains("declare i32 @sprintf(i8*, ...)"), "{ll}");
    }

    #[test]
    fn continuous_class_copy_emits_deep_copy_gate() {
        // P11d：连续类 var 声明 → DeepCopy 指令 → `hc_deep_copy_cont` 运行时门 +
        // 递归 `hc_deep_copy` helper；连续类名进字符串表供 strcmp 链匹配。
        let ll = gen(r#"[continuous]
class Point {
    x: f32,
    y: f32,
}
fn f() f32 {
    var p: Point = Point{ x = 1.0, y = 2.0 };
    var p2: Point = p;
    p2.x = 99.0;
    return p.x;
}"#);
        assert!(ll.contains("define %Value @hc_deep_copy_cont"), "{ll}");
        assert!(ll.contains("define %Value @hc_deep_copy"), "{ll}");
        assert!(ll.contains("call %Value @hc_deep_copy_cont"), "{ll}");
        assert!(ll.contains("c\"Point\\00\""), "{ll}"); // 连续类名进字符串表
        assert!(ll.contains("call i32 @strcmp"), "{ll}"); // 门 strcmp 链
    }

    #[test]
    fn non_continuous_class_no_deep_copy_inst() {
        // 非连续类 var 声明不发射 DeepCopy 指令（引用类型，按值赋值被语义层拒绝）；
        // 无连续类模块的门为恒等（仍定义，但无调用）。
        let ll = gen(r#"class Blob {
    x: i32,
    y: i32,
}
fn f() i32 {
    var b = Blob{ x = 1, y = 2 };
    return b.x;
}"#);
        // 无连续类 → 恒等门（不含递归 helper/strcmp 链），且无 DeepCopy 调用
        assert!(!ll.contains("call %Value @hc_deep_copy_cont"), "{ll}");
        assert!(!ll.contains("define %Value @hc_deep_copy("), "{ll}");
    }

    #[test]
    fn implicit_env_pi_seeded_in_entry() {
        // P11d：30-interface 的 Circle.area 读 `pi`——原生 LoadGlobal 此前为 Void → 0.0；
        // main/test 入口播种 Float(PI) 常量到 `@.h_globals` 槽位（对齐 IrRuntime::init）。
        let ll = gen(r#"fn area(r: f64) f64 {
    return pi * r * r;
}"#);
        // Float tag=3 + PI 位模式常量；对 @.h_globals 数组 GEP 后 store
        assert!(
            ll.contains("store %Value { i32 3, i128 4614256656552045848 }"),
            "{ll}"
        );
        assert!(
            ll.contains("getelementptr inbounds [10 x %Value], ptr @.h_globals, i64 0, i64 5"),
            "{ll}"
        );
        assert!(ll.contains("call %Value @hc_make_arr(i64 0)"), "{ll}"); // Vec/Deque/Table 空 Arr 播种
    }
}
