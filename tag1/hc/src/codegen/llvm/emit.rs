//! LLVM IR 发射：模块级代码生成与类型声明
//!
//! 定义：枚举：PrintSeg

use super::body::BodyEmitter;
use super::*;
use crate::ast::Type;
use crate::ir::{IrBinOp, IrConst, IrFunc, IrInst, IrModule};
use crate::runtime::errorcodes::ErrorCodeTable;
use std::collections::HashMap;
use std::fmt::Write as _;

pub(crate) fn build_slot_consts(f: &IrFunc) -> HashMap<usize, IrConst> {
    let mut m = HashMap::new();
    for inst in &f.body {
        if let IrInst::Const { temp, val } = inst {
            m.insert(*temp, val.clone());
        }
    }
    m
}

/// 将 AST 类型转换为简单字符串表示（用于类型槽表）
pub(crate) fn type_to_string(ty: &Type) -> String {
    match ty.strip() {
        Type::Named(name, _) => name.clone(),
        Type::Ptr(inner, mut_) => {
            let inner_s = type_to_string(inner);
            if *mut_ {
                format!("*mut {}", inner_s)
            } else {
                format!("*{}", inner_s)
            }
        }
        Type::Slice(inner, mut_) => {
            let inner_s = type_to_string(inner);
            if *mut_ {
                format!("&mut [{}]", inner_s)
            } else {
                format!("&[{}]", inner_s)
            }
        }
        Type::Optional(inner) => format!("?{}", type_to_string(inner)),
        Type::ErrorUnion(err, ok) => {
            let ok_s = type_to_string(ok);
            if let Some(e) = err {
                format!("{}!{}", type_to_string(e), ok_s)
            } else {
                format!("!{}", ok_s)
            }
        }
        Type::Tuple(items) => {
            let items: Vec<String> = items.iter().map(type_to_string).collect();
            format!("({})", items.join(", "))
        }
        Type::Array(n, inner) => format!("[{}]{}", n, type_to_string(inner)),
        Type::ComptimeInt(_) => "comptime_int".to_string(),
        Type::Infer => "infer".to_string(),
        Type::Owned(inner) => type_to_string(inner),
    }
}

/// 构建类型槽表：从函数参数类型 + 常量 + 指令传播推断每个槽的类型
pub(crate) fn build_type_slot_map(f: &IrFunc) -> HashMap<usize, String> {
    let mut m = HashMap::new();
    // 参数槽：从 param_ty 获取类型
    for (i, ps) in f.params.iter().enumerate() {
        if let Some(pt) = f.param_ty.get(i) {
            let ty_str = type_to_string(pt);
            m.insert(*ps, ty_str);
        }
    }
    // 多次传播：Load → Bin/Un → Store → Load 链需要多轮才能收敛
    for _pass in 0..3 {
        let mut changed = false;
        for inst in &f.body {
            match inst {
                IrInst::Const { temp, val } => {
                    let ty_str = match val {
                        IrConst::Int(_) => "i128",
                        IrConst::Float(_) => "f64",
                        IrConst::Bool(_) => "bool",
                        IrConst::Str(_) => "String",
                        IrConst::Void => "void",
                        IrConst::Null => "?void",
                        IrConst::Err { .. } => "error",
                        IrConst::End => "i64",
                    };
                    if m.insert(*temp, ty_str.to_string()).is_none() {
                        changed = true;
                    }
                }
                IrInst::Load { temp, slot } => {
                    if let Some(ty) = m.get(slot).cloned() {
                        if m.insert(*temp, ty).is_none() {
                            changed = true;
                        }
                    }
                }
                IrInst::Store { slot, temp } => {
                    if let Some(ty) = m.get(temp).cloned() {
                        if m.insert(*slot, ty).is_none() {
                            changed = true;
                        }
                    }
                }
                IrInst::Bin { op, temp, a, b } => {
                    // 比较运算符返回 bool（C8-1c）
                    match op {
                        IrBinOp::Eq
                        | IrBinOp::Ne
                        | IrBinOp::Lt
                        | IrBinOp::Le
                        | IrBinOp::Gt
                        | IrBinOp::Ge => {
                            if m.insert(*temp, "bool".to_string()).is_none() {
                                changed = true;
                            }
                        }
                        _ => {
                            let ta = m.get(a);
                            let tb = m.get(b);
                            if let (Some(ta), Some(tb)) = (ta, tb) {
                                if ta == tb {
                                    if m.insert(*temp, ta.clone()).is_none() {
                                        changed = true;
                                    }
                                } else if m.get(temp).is_none() {
                                    // 类型不同时优先用 a 类型
                                    if m.insert(*temp, ta.clone()).is_none() {
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
                IrInst::Un { op: _, temp, a } => {
                    if let Some(ty) = m.get(a).cloned() {
                        if m.insert(*temp, ty).is_none() {
                            changed = true;
                        }
                    }
                }
                IrInst::DeepCopy { temp, a } => {
                    if let Some(ty) = m.get(a).cloned() {
                        if m.insert(*temp, ty).is_none() {
                            changed = true;
                        }
                    }
                }
                IrInst::AddrSlot { temp, .. } | IrInst::AddrValue { temp, .. } => {
                    if m.insert(*temp, "*mut".to_string()).is_none() {
                        changed = true;
                    }
                }
                IrInst::Deref { temp, a } => {
                    if let Some(ty) = m.get(a).cloned() {
                        // 解引用：*mut T → T（去掉指针前缀）
                        let inner = if ty.starts_with("*mut ") {
                            ty[5..].to_string()
                        } else if ty.starts_with("*") {
                            ty[1..].to_string()
                        } else {
                            ty.clone()
                        };
                        if m.insert(*temp, inner).is_none() {
                            changed = true;
                        }
                    }
                }
                // C8-1c: CallBuiltin 已知返回类型
                IrInst::CallBuiltin { name, temp, .. } => {
                    let ty_str = match name.as_str() {
                        "@eq" | "@neq" | "@lt" | "@le" | "@gt" | "@ge" | "@is_null" | "@is_err"
                        | "@truthy" | "@not" | "@bool" => "bool",
                        "@len" | "@sizeOf" | "@alignOf" | "@offsetOf" => "i64",
                        _ => continue, // 未知内建跳过
                    };
                    if m.insert(*temp, ty_str.to_string()).is_none() {
                        changed = true;
                    }
                }
                // C8-1c: MakeClass → 类名
                IrInst::MakeClass { temp, ty, .. } => {
                    if m.insert(*temp, ty.clone()).is_none() {
                        changed = true;
                    }
                }
                // C8-1c: MakeEnum → 枚举名
                IrInst::MakeEnum { temp, name, .. } => {
                    if m.insert(*temp, name.clone()).is_none() {
                        changed = true;
                    }
                }
                // C8-1c: MakeArr → 数组
                IrInst::MakeArr { temp, .. } => {
                    if m.insert(*temp, "array".to_string()).is_none() {
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }
    m
}

/// 从槽常量子表读类型名（`@sizeOf(i32)` / `alloc.init(ABC)` 的类型位置参数）。
pub(crate) fn const_str_arg(
    slot_consts: &HashMap<usize, IrConst>,
    arg: Option<&usize>,
) -> Option<String> {
    match arg.and_then(|a| slot_consts.get(a)) {
        Some(IrConst::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

/// @sizeOf(T) 标量表（对齐 run_ir `scalar_size_ir`；用户 class/enum 无布局表 → None）
pub(crate) fn scalar_size_native(ty: &str) -> Option<usize> {
    match ty {
        "i8" | "u8" | "bool" => Some(1),
        "i16" | "u16" | "f16" => Some(2),
        "i32" | "u32" | "f32" => Some(4),
        "i64" | "u64" | "isize" | "usize" | "f64" => Some(8),
        "i128" | "u128" | "f128" => Some(16),
        "String" => Some(72),
        "Vec" | "Map" | "Deque" | "Table" | "Allocator" => Some(8),
        _ => None,
    }
}

/// @alignOf(T)（对齐 run_ir：i8/i16/i32/i128 显式，余下 size.min(8)，未知默认 8）
pub(crate) fn align_native(ty: &str) -> usize {
    match ty {
        "i8" | "u8" | "bool" => 1,
        "i16" | "u16" | "f16" => 2,
        "i32" | "u32" | "f32" => 4,
        "i128" | "u128" | "f128" => 16,
        "String" => 8,
        _ => scalar_size_native(ty).map(|s| s.min(8)).unwrap_or(8),
    }
}

/// @intCast 目标宽度范围（对齐 run_ir `int_width_bounds_ir`）
pub(crate) fn int_bounds_native(ty: &str) -> Option<(i128, i128)> {
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
pub(crate) fn is_io_print_name(name: &str) -> bool {
    matches!(
        name,
        "io.print" | "stdout.print" | "stderr.print" | "test_io.print"
    )
}

/// io.print 格式串段（对齐 oracle interp.rs:4042-4079 解析）。
pub(crate) enum PrintSeg {
    Lit(String),
    Arg { slot: Option<usize>, mode: u32 },
}

/// 解析格式串（B1/B3，2026-08-17）：`{}`→显示、`{d}`→十进制、`{x}`→十六进制小写、
/// `{X}`→十六进制大写、`{b}`→二进制、`{e}`→科学计数、`{s}`→显示；宽度/对齐/精度
/// （`{:8}`/`{:<6}`/`{:.2}`）原生暂不填充（值格式化不受影响——原生填充留后续）。
/// 其余字节为字面量。占位符无对应实参（参数不足）→ oracle 跳过（slot=None）。
pub(crate) fn parse_print_fmt(fmt: &str, args: &[usize]) -> Vec<PrintSeg> {
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

pub(crate) fn emit_func(
    out: &mut String,
    f: &IrFunc,
    idx: usize,
    n_caps: Option<usize>,
    strings: &[String],
    errors: &ErrorCodeTable,
    canon: &HashMap<String, Vec<usize>>,
    funcs: &[IrFunc],
    closures: &[IrFunc],
    gidx: &HashMap<String, usize>,
    prefix: &str,
    links: &HashMap<String, String>,
    ext_decls: &mut Vec<(String, usize)>,
) {
    let slot_consts = build_slot_consts(f);
    let is_closure = n_caps.is_some();
    let fn_tag = if is_closure { "hc_closure" } else { "hc_fn" };
    let _ = writeln!(out, "; {prefix}{fn_tag}{idx} = {}", f.name);
    // 闭包函数：第一个参数为 env 指针，显式参数跳过前 n_caps 个捕获参数
    let n = n_caps.unwrap_or(0);
    let params = if is_closure {
        let explicit: Vec<String> = (n..f.params.len())
            .map(|i| format!("%Value %p{i}"))
            .collect();
        if explicit.is_empty() {
            "%Value %env".to_string()
        } else {
            format!("%Value %env, {}", explicit.join(", "))
        }
    } else {
        (0..f.params.len())
            .map(|i| format!("%Value %p{i}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    // A1（ADR-0020）：`extern fn`——纯声明→ emit `declare` 而非 `define`
    if f.is_extern {
        let _ = writeln!(out, "declare %Value @\"{prefix}{fn_tag}{idx}\"({params})");
        return;
    }
    let _ = writeln!(out, "define %Value @\"{prefix}{fn_tag}{idx}\"({params}) {{");
    // 序言（槽数组 + 参数存槽）并入 entry 块（BodyEmitter 首个块即 entry）
    let type_slot_map = build_type_slot_map(f);
    let mut be = BodyEmitter::new(prefix, links, type_slot_map);
    let ns = f.n_slots;
    be.emit(format!("%slots = alloca [{ns} x %Value], align 16",));
    for i in 0..f.n_slots {
        be.emit(format!(
            "%sp.{i} = getelementptr inbounds [{ns} x %Value], [{ns} x %Value]* %slots, i32 0, i32 {i}",
        ));
    }
    // 闭包函数：从 env 结构加载捕获变量到对应槽
    if is_closure {
        let env_int = be.r();
        be.emit(format!("{env_int} = extractvalue %Value %env, 1"));
        let env_ptr = be.r();
        be.emit(format!("{env_ptr} = inttoptr i128 {env_int} to i8*"));
        let env_vals = be.r();
        be.emit(format!("{env_vals} = bitcast i8* {env_ptr} to %Value*"));
        for i in 0..n {
            let cptr = be.r();
            let cv = be.r();
            let slot = f.params[i];
            be.emit(format!(
                "{cptr} = getelementptr inbounds %Value, %Value* {env_vals}, i64 {i}"
            ));
            be.emit(format!("{cv} = load %Value, %Value* {cptr}"));
            be.emit(format!("store %Value {cv}, %Value* %sp.{slot}"));
        }
        // 只存显式参数（跳过前 n 个捕获参数）
        for (rel_i, &ps) in f.params.iter().enumerate().skip(n) {
            be.emit(format!("store %Value %p{rel_i}, %Value* %sp.{ps}"));
        }
    } else {
        for (i, ps) in f.params.iter().enumerate() {
            be.emit(format!("store %Value %p{i}, %Value* %sp.{ps}"));
        }
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
        be.inst(
            inst,
            strings,
            errors,
            canon,
            funcs,
            closures,
            gidx,
            &slot_consts,
        );
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
pub(crate) fn emit_ext_decls(out: &mut String, ext_decls: &[(String, usize)]) {
    for (sym, n) in ext_decls {
        let params = (0..*n)
            .map(|i| format!("%Value %d{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "declare %Value @\"{sym}\"({params})");
    }
}

/// K5（ADR-0014）：`export fn` 原生符号导出——链接器可见的干净符号。
/// 每个导出函数发射外部 thunk `define %Value @"{name}"(<%Value × N>)`，
/// 内部调用带 `prefix` 前缀的别名函数 `"{prefix}hc_fn{idx}"`（名称以 `%Value` 手性
/// 转发，保留 H 的标签+载荷调用约定）。模块末尾附符号清单注释（符号表断言目标）：
/// `; exports: a, b`；若导出 `_start` 则追加 `; entry: _start`（链接脚本入口钩子标记）。
pub(crate) fn emit_export_thunks(out: &mut String, module: &IrModule, prefix: &str) {
    let exports: Vec<(usize, &IrFunc)> = module
        .funcs
        .iter()
        .enumerate()
        .filter(|(_, f)| f.exported)
        .collect();
    if exports.is_empty() {
        return;
    }
    let names: Vec<&str> = exports.iter().map(|(_, f)| f.name.as_str()).collect();
    let _ = writeln!(out, "; exports: {}", names.join(", "));
    if names.iter().any(|n| *n == "_start") {
        out.push_str("; entry: _start\n");
    }
    for (idx, f) in exports {
        let params = (0..f.params.len())
            .map(|i| format!("%Value %p{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let args = (0..f.params.len())
            .map(|i| format!("%Value %p{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "; export thunk: {}", f.name);
        let _ = writeln!(out, "define %Value @\"{}\"({params}) {{", f.name);
        let _ = writeln!(
            out,
            "  %r = call %Value @\"{prefix}hc_fn{idx}\"({args})",
            prefix = prefix,
        );
        out.push_str("  ret %Value %r\n}\n\n");
    }
}

// ---------- main 包装（原生 CRT 入口） ----------

/// 发射对全部 `@__init__` 函数的调用（多文件合并 = 各模块 init 依次运行，entry 在前）。
/// `@__init__` 不在 func_index（不可被用户调用），此处按 funcs 声明序找到并执行；
/// 返回值是错误值 → 未处理错误到根（对齐 tree-walking `exec_decl_top` 失败即 panic）。
pub(crate) fn emit_init_calls(out: &mut String, module: &IrModule) {
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
pub(crate) fn emit_implicit_env_seed(out: &mut String, module: &IrModule) {
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

pub(crate) fn emit_main_wrapper(out: &mut String, module: &IrModule) {
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
            out.push_str("  %arglen = call i64 @strlen(i8* %argstr)\n");
            out.push_str("  %arglen_cap = icmp ugt i64 %arglen, 64\n");
            out.push_str("  %arglen_sat = select i1 %arglen_cap, i64 64, i64 %arglen\n");
            out.push_str("  %argraw = call noalias i8* @malloc(i64 72)\n");
            out.push_str("  %argsd = bitcast i8* %argraw to %StringData*\n");
            out.push_str(
                "  %argbuf_p = getelementptr %StringData, %StringData* %argsd, i64 0, i32 0\n",
            );
            out.push_str("  %argbuf = bitcast [64 x i8]* %argbuf_p to i8*\n");
            out.push_str("  call void @llvm.memcpy.p0i8.p0i8.i64(i8* %argbuf, i8* %argstr, i64 %arglen_sat, i1 false)\n");
            out.push_str(
                "  %arglen_p = getelementptr %StringData, %StringData* %argsd, i64 0, i32 1\n",
            );
            out.push_str("  store i64 %arglen_sat, i64* %arglen_p\n");
            out.push_str("  %argspi = ptrtoint %StringData* %argsd to i128\n");
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
/// abort(exit 1)，`error.SkipTest` 特判（错误码载荷匹配）→ 打印 [SKIP] 续跑下一个测试
/// （F1，对齐 oracle run_tests 值通道）。因断言失败即 abort，逐测试续跑需重做
/// assert→返回码通路——故 `hc test --mode=compile` 为文件粒度交叉验证（全绿 vs 有失败）。
pub(crate) fn emit_test_runner(out: &mut String, module: &IrModule, errors: &ErrorCodeTable) {
    // 注意：测试函数索引未必连续（@__init__ 等普通函数穿插），标签须用实际 func 索引
    let tests: Vec<usize> = module
        .funcs
        .iter()
        .enumerate()
        .filter(|(_, f)| f.is_test)
        .map(|(i, _)| i)
        .collect();
    // F1：error.SkipTest 在原生侧以「错误码载荷 == SkipTest 码」识别 → [SKIP] 续跑，
    // 对齐 oracle run_tests 值通道（return error.SkipTest → skipped+=1）
    let skip_code = errors.code_of("SkipTest");

    // 每个 [test] fn 的运行/通过/跳过标记字符串（模块级全局）
    for &idx in &tests {
        let f = &module.funcs[idx];
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
        if skip_code.is_some() {
            let skip = format!("[SKIP] {}", f.name);
            let _ = writeln!(
                out,
                "@.test.{idx}.skip = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
                skip.len() + 1,
                llvm_escape(skip.as_bytes())
            );
        }
    }

    out.push_str("define i32 @main(i32 %argc, i8** %argv) {\n");
    if tests.is_empty() {
        // 无 [test] 的库文件编译：跑器直接成功退出，不引用不存在的标签
        out.push_str("  ret i32 0\n}\n");
        return;
    }
    out.push_str("  %argvoid = load %Value, %Value* @.void_value\n");
    emit_implicit_env_seed(out, module);
    emit_init_calls(out, module);
    let _ = writeln!(out, "  br label %t{}", tests[0]);
    for (k, &idx) in tests.iter().enumerate() {
        let f = &module.funcs[idx];
        let next_label = if k + 1 < tests.len() {
            format!("t{}", tests[k + 1])
        } else {
            "tend".to_string()
        };
        let _ = writeln!(out, "t{idx}:");
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
            "  br i1 %is_err_{idx}, label %err_{idx}, label %pass_{idx}"
        );
        // err_{idx}：error.SkipTest（码匹配）→ [SKIP] 续跑；其余未处理错误 → abort(exit 1)
        let _ = writeln!(out, "err_{idx}:");
        if let Some(sc) = skip_code {
            let _ = writeln!(out, "  %code_{idx} = extractvalue %Value %r_{idx}, 1");
            let _ = writeln!(out, "  %skip_{idx} = icmp eq i128 %code_{idx}, {sc}");
            let _ = writeln!(
                out,
                "  br i1 %skip_{idx}, label %skp_{idx}, label %fail_{idx}"
            );
            let _ = writeln!(out, "skp_{idx}:");
            let skip = format!("[SKIP] {}", f.name);
            let sn = skip.len() + 1;
            let _ = writeln!(
                out,
                "  %skipp_{idx} = getelementptr inbounds [{sn} x i8], ptr @.test.{idx}.skip, i64 0, i64 0"
            );
            let _ = writeln!(out, "  call i32 @puts(i8* %skipp_{idx})");
            let _ = writeln!(out, "  br label %{next_label}");
        } else {
            let _ = writeln!(out, "  br label %fail_{idx}");
        }
        let _ = writeln!(out, "fail_{idx}:");
        out.push_str("  call void @hc_abort_unhandled()\n  unreachable\n");
        // pass_{idx}：[PASS] 续跑下一个测试
        let _ = writeln!(out, "pass_{idx}:");
        let pass = format!("[PASS] {}", f.name);
        let pn = pass.len() + 1;
        let _ = writeln!(
            out,
            "  %passp_{idx} = getelementptr inbounds [{pn} x i8], ptr @.test.{idx}.pass, i64 0, i64 0"
        );
        let _ = writeln!(out, "  call i32 @puts(i8* %passp_{idx})");
        let _ = writeln!(out, "  br label %{next_label}");
    }
    out.push_str("tend:\n  ret i32 0\n}\n");
}
