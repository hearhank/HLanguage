//! 函数体发射器：`%Value` SSA 形式 + 指令逐条发射。

use super::emit::*;
use super::*;
use crate::errorcodes::ErrorCodeTable;
use crate::ir::{IrBinOp, IrConst, IrFunc, IrInst, IrPattern, IrUnOp};
use std::collections::HashMap;
use std::fmt::Write as _;

pub(crate) struct BodyEmitter {
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
    pub(crate) ext_decls: Vec<(String, usize)>,
    /// C8-1a: 类型槽表——每个槽的推断类型（用于原生 ABI 判定、标量运算内联）
    pub(crate) type_slot_map: HashMap<usize, String>,
}

impl BodyEmitter {
    pub(crate) fn new(
        prefix: &str,
        links: &HashMap<String, String>,
        type_slot_map: HashMap<usize, String>,
    ) -> Self {
        BodyEmitter {
            ssa: 0,
            fresh: 0,
            blocks: Vec::new(),
            cur: "entry:\n".to_string(),
            terminated: false,
            prefix: prefix.to_string(),
            links: links.clone(),
            ext_decls: Vec::new(),
            type_slot_map,
        }
    }

    pub(crate) fn r(&mut self) -> String {
        let n = self.ssa;
        self.ssa += 1;
        format!("%r{n}")
    }

    pub(crate) fn fb(&mut self) -> String {
        let n = self.fresh;
        self.fresh += 1;
        format!("fb{n}")
    }

    pub(crate) fn emit(&mut self, line: String) {
        self.cur.push_str("  ");
        self.cur.push_str(&line);
        self.cur.push('\n');
    }

    pub(crate) fn term(&mut self, line: String) {
        self.emit(line);
        self.terminated = true;
        self.close_block();
    }

    pub(crate) fn close_block(&mut self) {
        if !self.terminated {
            self.cur.push_str("  unreachable\n");
        }
        self.blocks.push(std::mem::take(&mut self.cur));
        self.terminated = false;
    }

    pub(crate) fn label(&mut self, id: usize) {
        if !self.terminated {
            let _ = writeln!(self.cur, "  br label %L{id}");
            self.terminated = true;
        }
        self.close_block();
        self.cur = format!("L{id}:\n");
        self.terminated = false;
    }

    pub(crate) fn cond_br(&mut self, cond: &str, label: usize) {
        let fb = self.fb();
        self.term(format!("br i1 {cond}, label %L{label}, label %{fb}"));
        self.cur = format!("{fb}:\n");
        self.terminated = false;
    }

    /// 特性未支持硬中止：当前块 br 到错误块（hc_abort_{key} + unreachable），
    /// 后续指令落到不可达续块（LLVM 允许；运行即 abort，杜绝静默误编译）。
    pub(crate) fn abort_feature(&mut self, key: &str) {
        let l = self.fb();
        self.term(format!("br label %{l}"));
        self.blocks.push(format!(
            "{l}:\n  call void @hc_abort_{key}()\n  unreachable\n"
        ));
        self.cur = format!("{l}.cont:\n");
        self.terminated = false;
    }

    pub(crate) fn finish(mut self) -> String {
        if !self.cur.is_empty() {
            self.close_block();
        }
        self.blocks.join("")
    }

    pub(crate) fn build_store(&mut self, temp: usize, tag: i32, data: String) {
        let v0 = self.r();
        self.emit(format!(
            "{v0} = insertvalue %Value {{ i32 0, i128 0 }}, i32 {tag}, 0"
        ));
        let v1 = self.r();
        self.emit(format!("{v1} = insertvalue %Value {v0}, i128 {data}, 1"));
        self.emit(format!("store %Value {v1}, %Value* %sp.{temp}"));
    }

    pub(crate) fn const_(
        &mut self,
        temp: usize,
        val: &IrConst,
        strings: &[String],
        errors: &ErrorCodeTable,
    ) {
        let v = self.const_value(val, strings, errors);
        self.emit(format!("store %Value {v}, %Value* %sp.{temp}"));
    }

    /// 常量 → `%Value` SSA 值（Str 取全局字符串地址；余下 tag+data 直接 insertvalue）
    pub(crate) fn const_value(
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

    pub(crate) fn emit_bool_store(&mut self, temp: usize, cond: &str) {
        let b = self.r();
        self.emit(format!("{b} = call %Value @hc_bool(i1 {cond})"));
        self.emit(format!("store %Value {b}, %Value* %sp.{temp}"));
    }

    pub(crate) fn bin(&mut self, op: IrBinOp, temp: usize, a: usize, b: usize) {
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

    pub(crate) fn un(&mut self, op: IrUnOp, temp: usize, a: usize) {
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
    pub(crate) fn arglist(&mut self, args: &[usize]) -> String {
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

    pub(crate) fn call(
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
    pub(crate) fn call_math(&mut self, field: &str, args: &[usize], temp: usize) {
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
            "sqrt" => {
                self.emit_math_unop_inline("call double @llvm.sqrt.f64(double %f)", args, temp)
            }
            "abs" => {
                self.emit_math_unop_inline("call double @llvm.fabs.f64(double %f)", args, temp)
            }
            "floor" => {
                self.emit_math_unop_inline("call double @llvm.floor.f64(double %f)", args, temp)
            }
            "ceil" => {
                self.emit_math_unop_inline("call double @llvm.ceil.f64(double %f)", args, temp)
            }
            "round" => {
                self.emit_math_unop_inline("call double @llvm.round.f64(double %f)", args, temp)
            }
            "pow" => {
                self.emit_math_unop_inline("call double @pow(double %f, double %f)", args, temp)
            }
            _ => self.abort_feature("builtin"),
        }
    }

    /// 内联数值一元运算：提取 `%Value` 中的 f64 值（支持 Int→f64 强制转换），
    /// 应用 `call_expr`（LLVM 原生内建或 libm 调用，`%f` 为双精度值），
    /// 结果装箱为 `T_FLOAT` 的 `%Value` 存入目标槽。
    /// 替代 `math_unop_helper` 运行时 helper，直接发射原生 LLVM 指令。
    pub(crate) fn emit_math_unop_inline(&mut self, call_expr: &str, args: &[usize], temp: usize) {
        let Some(&vslot) = args.first() else {
            self.abort_feature("builtin");
            return;
        };
        let v = self.r();
        self.emit(format!("{v} = load %Value, %Value* %sp.{vslot}"));
        let tag = self.r();
        self.emit(format!("{tag} = extractvalue %Value {v}, 0"));
        let data = self.r();
        self.emit(format!("{data} = extractvalue %Value {v}, 1"));
        let is_int = self.r();
        self.emit(format!("{is_int} = icmp eq i32 {tag}, {T_INT}"));
        let dt = self.r();
        self.emit(format!("{dt} = trunc i128 {data} to i64"));
        let asf = self.r();
        self.emit(format!("{asf} = sitofp i64 {dt} to double"));
        let raw = self.r();
        self.emit(format!("{raw} = bitcast i64 {dt} to double"));
        let f = self.r();
        self.emit(format!(
            "{f} = select i1 {is_int}, double {asf}, double {raw}"
        ));
        // 代入 call_expr：将 `%f` 替换为实际寄存器名
        // 对 pow 需要两个 %f 参数，全部替换
        let call_str = call_expr.replace("%f", &f);
        let r = self.r();
        self.emit(format!("{r} = {call_str}"));
        let bits = self.r();
        self.emit(format!("{bits} = bitcast double {r} to i64"));
        let z = self.r();
        self.emit(format!("{z} = zext i64 {bits} to i128"));
        self.build_store(temp, T_FLOAT, z);
    }

    pub(crate) fn call_builtin(
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
        // K2：@volatileLoad(ptr)——读穿指针；@volatileStore(ptr, v)——写穿指针。
        // 经 hc_volatile_load/hc_volatile_store（LLVM `load volatile`/`store volatile`，
        // 防优化掉副作用，MMIO 场景）。调用本身不标 readonly → 外部调用点亦不可省略。
        if name == "@volatileLoad" {
            let Some(&vslot) = args.first() else {
                self.abort_feature("builtin");
                return;
            };
            let v = self.r();
            self.emit(format!("{v} = load %Value, %Value* %sp.{vslot}"));
            // 内联：extractvalue data → inttoptr → load volatile
            let d = self.r();
            self.emit(format!("{d} = extractvalue %Value {v}, 1"));
            let pp = self.r();
            self.emit(format!("{pp} = inttoptr i128 {d} to %Value*"));
            let pv = self.r();
            self.emit(format!("{pv} = load volatile %Value, %Value* {pp}"));
            self.emit(format!("store %Value {pv}, %Value* %sp.{temp}"));
            return;
        }
        if name == "@volatileStore" {
            if args.len() != 2 {
                self.abort_feature("builtin");
                return;
            }
            let p = self.r();
            self.emit(format!("{p} = load %Value, %Value* %sp.{}", args[0]));
            let v = self.r();
            self.emit(format!("{v} = load %Value, %Value* %sp.{}", args[1]));
            // 内联：extractvalue data → inttoptr → store volatile
            let pd = self.r();
            self.emit(format!("{pd} = extractvalue %Value {p}, 1"));
            let pp = self.r();
            self.emit(format!("{pp} = inttoptr i128 {pd} to %Value*"));
            self.emit(format!("store volatile %Value {v}, %Value* {pp}"));
            return;
        }
        // K4：@ptrFromInt(addr)——整数载荷 → T_PTR 标记（虚拟指针）；@intFromPtr(p)——指针
        // 载荷 → T_INT 标记。tag1 指针载荷 = i128 ptrtoint 地址，两内建 = 载荷搬运
        // （extractvalue i128 载荷 → 以目标 tag 重建 %Value）。
        if name == "@ptrFromInt" {
            let Some(&nslot) = args.first() else {
                self.abort_feature("builtin");
                return;
            };
            let n = self.r();
            self.emit(format!("{n} = load %Value, %Value* %sp.{nslot}"));
            let nd = self.r();
            self.emit(format!("{nd} = extractvalue %Value {n}, 1"));
            self.build_store(temp, T_PTR, nd);
            return;
        }
        if name == "@intFromPtr" {
            let Some(&pslot) = args.first() else {
                self.abort_feature("builtin");
                return;
            };
            let p = self.r();
            self.emit(format!("{p} = load %Value, %Value* %sp.{pslot}"));
            let pd = self.r();
            self.emit(format!("{pd} = extractvalue %Value {p}, 1"));
            self.build_store(temp, T_INT, pd);
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
    pub(crate) fn emit_unop_helper(&mut self, helper: &str, args: &[usize], temp: usize) {
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
    pub(crate) fn emit_binop_helper(&mut self, helper: &str, args: &[usize], temp: usize) {
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
    pub(crate) fn call_alloc_init(
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
    pub(crate) fn call_print(
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
    pub(crate) fn call_method(
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
    pub(crate) fn call_coll_method(&mut self, temp: usize, method: &str, dv: &str, args: &[usize]) {
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
    pub(crate) fn call_method_user(
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

    pub(crate) fn ret(&mut self, slot: usize) {
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

    pub(crate) fn ret_void(&mut self) {
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

    pub(crate) fn inst(
        &mut self,
        inst: &IrInst,
        strings: &[String],
        errors: &ErrorCodeTable,
        canon: &HashMap<String, Vec<usize>>,
        funcs: &[IrFunc],
        closures: &[IrFunc],
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
                let tag = self.r();
                self.emit(format!("{tag} = extractvalue %Value {v}, 0"));
                let c = self.r();
                self.emit(format!("{c} = icmp eq i32 {tag}, {T_NULL}"));
                self.cond_br(&c, *label);
            }
            IrInst::JumpIfErr { temp, label } => {
                let v = self.r();
                self.emit(format!("{v} = load %Value, %Value* %sp.{temp}"));
                let tag = self.r();
                self.emit(format!("{tag} = extractvalue %Value {v}, 0"));
                let c = self.r();
                self.emit(format!("{c} = icmp eq i32 {tag}, {T_ERR}"));
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
            // K1 union 原生后端临时取舍（ADR-0014）：union 写字段需运行时字节重解释同步
            // 其余字段（tag 感知截断/位重读）。与闭包/notcallable 同类——响亮拒绝，禁止
            // 静默误编译。所有 union 值必经 MakeClass + UnionSync（构造即触发本中止），
            // 故原生 union 程序在首个字面量处中止，绝不产生错误字段布局的可观察行为。
            IrInst::UnionSync { .. } => self.abort_feature("builtin"),
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
            // ---- Phase 8 原生 ABI：闭包/函数引用/间接调用 ----
            IrInst::MakeClosure {
                temp,
                func,
                captures,
                is_move: _,
                is_mut: _,
            } => {
                let n_caps = captures.len();
                let env_size = n_caps * 16; // 16 bytes per %Value
                                            // 分配 env 结构
                let env_ptr = self.r();
                self.emit(format!("{env_ptr} = call i8* @malloc(i64 {env_size})"));
                let env_vals = self.r();
                self.emit(format!("{env_vals} = bitcast i8* {env_ptr} to %Value*"));
                // 逐个拷贝捕获变量到 env 结构
                for (i, (_, slot)) in captures.iter().enumerate() {
                    let v = self.r();
                    self.emit(format!("{v} = load %Value, %Value* %sp.{slot}"));
                    let cp = self.r();
                    self.emit(format!(
                        "{cp} = getelementptr inbounds %Value, %Value* {env_vals}, i64 {i}"
                    ));
                    self.emit(format!("store %Value {v}, %Value* {cp}"));
                }
                // 分配胖闭包结构 { i8* fn_ptr, i8* env_ptr } = 16 bytes
                let fc_ptr = self.r();
                self.emit(format!("{fc_ptr} = call i8* @malloc(i64 16)"));
                let fc_fn = self.r();
                self.emit(format!("{fc_fn} = bitcast i8* {fc_ptr} to i8**"));
                // 获取闭包函数指针
                if let Some(cf) = closures.get(*func) {
                    let n_params = cf.params.len();
                    let param_types = (0..n_params)
                        .map(|_| "%Value")
                        .collect::<Vec<_>>()
                        .join(", ");
                    let fn_type = format!("%Value ({param_types})");
                    let fn_i8 = self.r();
                    let fn_name = format!("{}hc_closure{func}", self.prefix);
                    self.emit(format!(
                        "{fn_i8} = bitcast {fn_type}* @\"{fn_name}\" to i8*"
                    ));
                    self.emit(format!("store i8* {fn_i8}, i8** {fc_fn}"));
                } else {
                    self.abort_feature("notcallable");
                    return;
                }
                // 存储 env 指针到胖闭包结构
                let fc_env = self.r();
                self.emit(format!(
                    "{fc_env} = getelementptr inbounds i8*, i8** {fc_fn}, i64 1"
                ));
                self.emit(format!("store i8* {env_ptr}, i8** {fc_env}"));
                // 存储胖闭包指针到 T_CLOSURE 值
                let fc_int = self.r();
                self.emit(format!("{fc_int} = ptrtoint i8* {fc_ptr} to i128"));
                self.build_store(*temp, T_CLOSURE, fc_int);
            }
            IrInst::FnRef { temp, name } => {
                let Some(candidates) = canon.get(name) else {
                    // 函数未找到：运行时错误
                    let res = self.r();
                    self.emit(format!("{res} = call %Value @hc_no_function()"));
                    self.emit(format!("store %Value {res}, %Value* %sp.{temp}"));
                    return;
                };
                let target = candidates[0];
                let n_params = funcs[target].params.len();
                let param_types = (0..n_params)
                    .map(|_| "%Value")
                    .collect::<Vec<_>>()
                    .join(", ");
                let fn_type = format!("%Value ({param_types})");
                let fn_ptr = self.r();
                let fn_name = format!("{}hc_fn{target}", self.prefix);
                self.emit(format!(
                    "{fn_ptr} = ptrtoint {fn_type}* @\"{fn_name}\" to i128"
                ));
                self.build_store(*temp, T_FN, fn_ptr);
            }
            IrInst::CallIndirect { temp, callee, args } => {
                let callee_v = self.r();
                self.emit(format!("{callee_v} = load %Value, %Value* %sp.{callee}"));
                let tag = self.r();
                self.emit(format!("{tag} = extractvalue %Value {callee_v}, 0"));
                let is_fn = self.r();
                self.emit(format!("{is_fn} = icmp eq i32 {tag}, {T_FN}"));
                let fn_label = self.fb();
                let done_label = self.fb();
                let fb = self.fb();
                self.term(format!("br i1 {is_fn}, label %L{fn_label}, label %{fb}"));
                // T_FN 路径：提取函数指针，inttoptr，调用
                self.cur = format!("L{fn_label}:\n");
                self.terminated = false;
                let payload = self.r();
                self.emit(format!("{payload} = extractvalue %Value {callee_v}, 1"));
                let n = args.len();
                let param_types = (0..n).map(|_| "%Value").collect::<Vec<_>>().join(", ");
                let fn_type = format!("%Value ({param_types})");
                let fn_ptr = self.r();
                self.emit(format!("{fn_ptr} = inttoptr i128 {payload} to {fn_type}*"));
                let arglist = args
                    .iter()
                    .map(|a| format!("%Value %sp.{a}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let fn_r = self.r();
                self.emit(format!("{fn_r} = call {fn_type} {fn_ptr}({arglist})"));
                self.emit(format!("store %Value {fn_r}, %Value* %sp.{temp}"));
                self.term(format!("br label %L{done_label}"));
                // T_CLOSURE 路径：提取胖闭包，加载 fn/env，调用
                self.cur = format!("{fb}:\n");
                self.terminated = false;
                let cl_payload = self.r();
                self.emit(format!("{cl_payload} = extractvalue %Value {callee_v}, 1"));
                let fc_i8 = self.r();
                self.emit(format!("{fc_i8} = inttoptr i128 {cl_payload} to i8*"));
                let fc_fn_p = self.r();
                self.emit(format!("{fc_fn_p} = bitcast i8* {fc_i8} to i8**"));
                let fn_i8 = self.r();
                self.emit(format!("{fn_i8} = load i8*, i8** {fc_fn_p}"));
                let fc_env_p = self.r();
                self.emit(format!(
                    "{fc_env_p} = getelementptr inbounds i8*, i8** {fc_fn_p}, i64 1"
                ));
                let env_i8 = self.r();
                self.emit(format!("{env_i8} = load i8*, i8** {fc_env_p}"));
                // 构造 env %Value（T_PTR tag）
                let env_int = self.r();
                self.emit(format!("{env_int} = ptrtoint i8* {env_i8} to i128"));
                let env_v0 = self.r();
                self.emit(format!(
                    "{env_v0} = insertvalue %Value {{ i32 0, i128 0 }}, i32 {T_PTR}, 0"
                ));
                let env_v1 = self.r();
                self.emit(format!(
                    "{env_v1} = insertvalue %Value {env_v0}, i128 {env_int}, 1"
                ));
                // 转换 fn 到正确函数类型（1 + N 显式参数）
                let n_cl = args.len() + 1;
                let param_types_cl = (0..n_cl).map(|_| "%Value").collect::<Vec<_>>().join(", ");
                let fn_type_cl = format!("%Value ({param_types_cl})");
                let fn_ptr_cl = self.r();
                self.emit(format!(
                    "{fn_ptr_cl} = bitcast i8* {fn_i8} to {fn_type_cl}*"
                ));
                let arglist_cl = format!(
                    "%Value {env_v1}, {}",
                    args.iter()
                        .map(|a| format!("%Value %sp.{a}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let cl_r = self.r();
                self.emit(format!(
                    "{cl_r} = call {fn_type_cl} {fn_ptr_cl}({arglist_cl})"
                ));
                self.emit(format!("store %Value {cl_r}, %Value* %sp.{temp}"));
                self.term(format!("br label %L{done_label}"));
                // 完成
                self.cur = format!("L{done_label}:\n");
                self.terminated = false;
            }
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
