use super::*;

/// 指令是否含控制流/登记副作用——defer 体内出现即硬错误（带 label 的跳转指令
/// 在退出点重复发射会冲突；PushDefer/PopDefer 为 defer 登记副作用）。
pub(crate) fn is_control_flow_inst(i: &IrInst) -> bool {
    matches!(
        i,
        IrInst::Jump { .. }
            | IrInst::JumpIf { .. }
            | IrInst::JumpIfNot { .. }
            | IrInst::JumpIfNull { .. }
            | IrInst::JumpIfErr { .. }
            | IrInst::Label { .. }
            | IrInst::Return { .. }
            | IrInst::ReturnVoid
            | IrInst::PushDefer { .. }
            | IrInst::PopDefer { .. }
            | IrInst::JumpIfNotDefer { .. }
    )
}

/// switch 模式 → IR 模式（`Else` → None：不发射 MatchTest，由兜底臂处理）。
pub(crate) fn to_ir_pattern(p: &SwitchPattern) -> Option<IrPattern> {
    match p {
        SwitchPattern::Error(s) => Some(IrPattern::Error(s.clone())),
        SwitchPattern::Ident(s) => Some(IrPattern::Ident(s.clone())),
        SwitchPattern::Int(s) => Some(IrPattern::Int(parse_int_lit(s))),
        SwitchPattern::Float(s) => Some(IrPattern::Float(s.parse().unwrap_or(0.0))),
        SwitchPattern::Str(s) => Some(IrPattern::Str(s.clone())),
        SwitchPattern::Char(c) => Some(IrPattern::Char(*c)),
        SwitchPattern::Else => None,
    }
}

pub(crate) fn to_assign_binop(op: AssignOp) -> IrBinOp {
    match op {
        AssignOp::Add => IrBinOp::Add,
        AssignOp::Sub => IrBinOp::Sub,
        AssignOp::Mul => IrBinOp::Mul,
        AssignOp::Div => IrBinOp::Div,
        AssignOp::BitOr => IrBinOp::BitOr,
        AssignOp::BitAnd => IrBinOp::BitAnd,
        AssignOp::BitXor => IrBinOp::BitXor,
        AssignOp::Set => unreachable!("Set 单独处理"),
    }
}

pub(crate) fn to_ir_binop(op: BinOp) -> IrBinOp {
    match op {
        BinOp::Add => IrBinOp::Add,
        BinOp::Sub => IrBinOp::Sub,
        BinOp::Mul => IrBinOp::Mul,
        BinOp::Div => IrBinOp::Div,
        BinOp::Mod => IrBinOp::Mod,
        BinOp::EucMod => IrBinOp::EucMod,
        BinOp::BitAnd => IrBinOp::BitAnd,
        BinOp::BitOr => IrBinOp::BitOr,
        BinOp::BitXor => IrBinOp::BitXor,
        BinOp::Shl => IrBinOp::Shl,
        BinOp::Shr => IrBinOp::Shr,
        BinOp::Eq => IrBinOp::Eq,
        BinOp::Ne => IrBinOp::Ne,
        BinOp::Lt => IrBinOp::Lt,
        BinOp::Le => IrBinOp::Le,
        BinOp::Gt => IrBinOp::Gt,
        BinOp::Ge => IrBinOp::Ge,
        // 短路/区间在 lower_expr 单独处理，此处为不可达兜底
        BinOp::And | BinOp::Or | BinOp::Range => IrBinOp::Eq,
    }
}

/// 断言内建（IR 参考解释器实现）
pub(crate) fn is_assert_builtin(name: &str) -> bool {
    matches!(
        name,
        "expect" | "expect_eq" | "expect_neq" | "expect_error" | "expect_eq_slices"
    )
}

/// 组 F：四模式共享容器类型名（对齐 oracle interp.rs `is_four_mode_type`）
pub(crate) fn is_four_mode_type_ir(name: &str) -> bool {
    matches!(name, "OneToOne" | "OneToMany" | "ManyToOne" | "ManyToMany")
}

/// 自由内建（非 `@` 前缀；测试内隐式可用，普通函数体按名路由到 `CallBuiltin`）。
/// 对齐 oracle `call_builtin`（interp.rs:2911）的用户可调内建面。
pub(crate) fn is_free_builtin(name: &str) -> bool {
    matches!(
        name,
        // 内存/复制
        "box" | "copy"
            // 数值工具
            | "sqrt" | "min" | "max"
            // 格式辅助（M5.3 serialize）
            | "fmt_int" | "fmt_float"
            // 字节工具
            | "read_u64_le"
            // 算法
            | "sort" | "binary_search"
            // 解析器辅助（71-recursive-parser）
            | "skip_space" | "peek" | "advance" | "is_digit" | "parse_number"
            // 文本解析
            | "parse_int" | "parse_float"
            // 组 G 线程（E2.2）：spawn(f, args...) o Thread(T)——协作式延迟执行
            | "spawn"
            // 组 F：四模式类型实例化 OneToOne(i32) → 空容器标记（init 构造真实容器）
            | "OneToOne" | "OneToMany" | "ManyToOne" | "ManyToMany"
    )
}

/// `@` 内建的「类型位置」参数（类型名以 `Const Str` 编码，运行时按名解析）。
/// 对齐 oracle `@sizeOf/@alignOf/@offsetOf/@intCast/@enumFromInt/@ptrCast/@alignCast`
/// 从 `Expr::Ident` 读类型名（interp.rs:3009-3030, 3068-3093）。
pub(crate) fn is_type_arg_pos(name: &str, i: usize) -> bool {
    match name {
        "@sizeOf" | "@alignOf" => i == 0,
        "@offsetOf" => i == 0 || i == 1,
        "@intCast" | "@enumFromInt" | "@ptrCast" | "@alignCast" => i == 0,
        // 组 F（Q-S3）：@atomic* 首参为类型名 T（@atomicLoad(T,p,order) 等）
        "@atomicLoad" | "@atomicStore" | "@atomicRmw" => i == 0,
        // alloc.init(ABC) / arena.init(ABC)：类型名参数（运行时按名建空实例）
        "alloc.init" | "arena.init" => i == 0,
        // 组 F：四模式类型实例化 OneToOne(i32) 的类型参数——降级 Const Str、
        // 运行时 call_builtin 忽略（对齐 oracle 的 eval_call 标记短路）
        "OneToOne" | "OneToMany" | "ManyToOne" | "ManyToMany" => i == 0,
        // math.nan/math.inf/math.inf_neg(f64)：类型名参数（运行时忽略，仅指示宽度）
        "math.nan" | "math.inf" | "math.inf_neg" => i == 0,
        _ => false,
    }
}

/// 整数/浮点字面量解析（后缀、下划线、进制）
pub(crate) fn parse_int_lit(text: &str) -> i128 {
    let cleaned: String = text
        .chars()
        .take_while(|c| {
            c.is_ascii_digit()
                || matches!(c, 'x' | 'X' | 'b' | 'B' | 'o' | 'O' | 'a'..='f' | 'A'..='F' | '_')
        })
        .collect();
    let cleaned = cleaned.replace('_', "");
    let (radix, digits) = if let Some(r) = cleaned.strip_prefix("0x").or(cleaned.strip_prefix("0X"))
    {
        (16u32, r)
    } else if let Some(r) = cleaned.strip_prefix("0b").or(cleaned.strip_prefix("0B")) {
        (2u32, r)
    } else if let Some(r) = cleaned.strip_prefix("0o").or(cleaned.strip_prefix("0O")) {
        (8u32, r)
    } else {
        (10u32, cleaned.as_str())
    };
    i128::from_str_radix(digits, radix).unwrap_or(0)
}

/// E1.2 组 D D5：一元常量运算（comptime 值函数体折叠）。对齐 oracle 语义
/// （interp.rs:2495-2510）：Neg 仅数值、Not 任意值转布尔、BitNot 仅整数。
/// 不支持 → None（折叠回退既有调用路径）。
pub(crate) fn const_unary(op: UnaryOp, v: &IrConst) -> Option<IrConst> {
    match op {
        UnaryOp::Neg => match v {
            IrConst::Int(i) => Some(IrConst::Int(-i)),
            IrConst::Float(f) => Some(IrConst::Float(-f)),
            _ => None,
        },
        UnaryOp::Not => match v {
            IrConst::Bool(b) => Some(IrConst::Bool(!b)),
            IrConst::Int(i) => Some(IrConst::Bool(*i == 0)),
            IrConst::Float(f) => Some(IrConst::Bool(*f == 0.0)),
            _ => None,
        },
        UnaryOp::BitNot => match v {
            IrConst::Int(i) => Some(IrConst::Int(!i)),
            _ => None,
        },
    }
}

/// E1.2 组 D D5：二元常量运算（comptime 值函数体折叠）。对齐 oracle `binop_values`/
/// `arith`（interp.rs:2811-2933）：Int 溢出回 None（回退）、除零回 None、Int/Float
/// 混算提升 Int→Float、比较按值序。不支持 → None。
pub(crate) fn const_binop(op: BinOp, l: &IrConst, r: &IrConst) -> Option<IrConst> {
    use IrConst::{Bool, Float, Int};
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::EucMod => {
            match (l, r) {
                (Int(a), Int(b)) => {
                    let v = match op {
                        BinOp::Add => a.checked_add(*b),
                        BinOp::Sub => a.checked_sub(*b),
                        BinOp::Mul => a.checked_mul(*b),
                        BinOp::Div => (*b != 0).then(|| a / b),
                        BinOp::Mod => (*b != 0).then(|| a % b),
                        BinOp::EucMod => (*b != 0).then(|| a.rem_euclid(*b)),
                        _ => None,
                    };
                    v.map(Int)
                }
                (Float(a), Float(b)) => {
                    let v = match op {
                        BinOp::Add => a + b,
                        BinOp::Sub => a - b,
                        BinOp::Mul => a * b,
                        BinOp::Div => a / b,
                        BinOp::Mod | BinOp::EucMod => a % b,
                        _ => return None,
                    };
                    Some(Float(v))
                }
                (Int(a), Float(_)) => const_binop(op, &Float(*a as f64), r),
                (Float(_), Int(b)) => const_binop(op, l, &Float(*b as f64)),
                _ => None,
            }
        }
        BinOp::Eq => Some(Bool(const_eq(l, r))),
        BinOp::Ne => Some(Bool(!const_eq(l, r))),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let lt = const_lt(l, r)?;
            let eq = const_eq(l, r);
            let v = match op {
                BinOp::Lt => lt,
                BinOp::Le => lt || eq,
                BinOp::Gt => !lt && !eq,
                BinOp::Ge => !lt || eq,
                _ => unreachable!(),
            };
            Some(Bool(v))
        }
        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => match (l, r) {
            (Int(a), Int(b)) => {
                let v = match op {
                    BinOp::BitAnd => a & b,
                    BinOp::BitOr => a | b,
                    BinOp::BitXor => a ^ b,
                    BinOp::Shl => {
                        if *a >= 0 && *a <= u64::MAX as i128 && *b < 64 {
                            ((*a as u64).wrapping_shl(*b as u32)) as i128
                        } else {
                            a << b
                        }
                    }
                    BinOp::Shr => {
                        if *a >= 0 && *a <= u64::MAX as i128 && *b < 64 {
                            ((*a as u64).wrapping_shr(*b as u32)) as i128
                        } else {
                            a >> b
                        }
                    }
                    _ => return None,
                };
                Some(Int(v))
            }
            _ => None,
        },
        _ => None,
    }
}

/// D5：常量相等比较（Int/Float 互比提升、Bool、Str、Null、Void）。
pub(crate) fn const_eq(l: &IrConst, r: &IrConst) -> bool {
    use IrConst::{Bool, Float, Int, Null, Str, Void};
    match (l, r) {
        (Int(a), Int(b)) => a == b,
        (Float(a), Float(b)) => a == b,
        (Int(a), Float(b)) => *a as f64 == *b,
        (Float(a), Int(b)) => *a == *b as f64,
        (Bool(a), Bool(b)) => a == b,
        (Str(a), Str(b)) => a == b,
        (Null, Null) => true,
        (Void, Void) => true,
        _ => l == r,
    }
}

/// D5：常量小于比较（Int/Float 互比提升；其他不支持 → None）。
pub(crate) fn const_lt(l: &IrConst, r: &IrConst) -> Option<bool> {
    use IrConst::{Float, Int};
    match (l, r) {
        (Int(a), Int(b)) => Some(a < b),
        (Float(a), Float(b)) => Some(a < b),
        (Int(a), Float(b)) => Some((*a as f64) < *b),
        (Float(a), Int(b)) => Some(*a < *b as f64),
        _ => None,
    }
}

// ---------- IR 参考解释器（M3.1：唯一语义源的语义定义） ----------

pub(crate) fn binop(op: IrBinOp, ctx: &Ctx, a: &IrValue, b: &IrValue) -> R<IrValue> {
    match op {
        IrBinOp::Add
        | IrBinOp::Sub
        | IrBinOp::Mul
        | IrBinOp::Div
        | IrBinOp::Mod
        | IrBinOp::EucMod => {
            use IrValue::*;
            match (a, b) {
                // 整数：溢出 → Overflow，除/模零 → DivisionByZero（对齐 tree-walking arith）
                (Int(x), Int(y)) => {
                    let v = match op {
                        IrBinOp::Add => x.checked_add(*y),
                        IrBinOp::Sub => x.checked_sub(*y),
                        IrBinOp::Mul => x.checked_mul(*y),
                        IrBinOp::Div => {
                            if *y == 0 {
                                return R::Err(IrError::msg("DivisionByZero", "division by zero"));
                            }
                            Some(x / y)
                        }
                        IrBinOp::Mod => {
                            if *y == 0 {
                                return R::Err(IrError::msg("DivisionByZero", "modulo by zero"));
                            }
                            Some(x % y)
                        }
                        IrBinOp::EucMod => {
                            if *y == 0 {
                                return R::Err(IrError::msg(
                                    "DivisionByZero",
                                    "euclidean modulo by zero",
                                ));
                            }
                            Some(x.rem_euclid(*y))
                        }
                        _ => None,
                    };
                    match v {
                        Some(v) => Ok(Int(v)),
                        None => R::Err(IrError::msg("Overflow", "integer overflow")),
                    }
                }
                // 混合/浮点：IEEE 语义，除零 = inf（对齐 tree-walking arith Float 分支）
                (Int(x), Float(y)) | (Float(y), Int(x)) => {
                    let (x, y) = (x.clone(), y.clone());
                    let v = match op {
                        IrBinOp::Add => x as f64 + y,
                        IrBinOp::Sub => x as f64 - y,
                        IrBinOp::Mul => x as f64 * y,
                        IrBinOp::Div => x as f64 / y,
                        IrBinOp::Mod | IrBinOp::EucMod => (x as f64) % y,
                        _ => 0.0,
                    };
                    Ok(Float(v))
                }
                (Float(x), Float(y)) => {
                    let v = match op {
                        IrBinOp::Add => x + y,
                        IrBinOp::Sub => x - y,
                        IrBinOp::Mul => x * y,
                        IrBinOp::Div => x / y,
                        IrBinOp::Mod | IrBinOp::EucMod => x % y,
                        _ => 0.0,
                    };
                    Ok(Float(v))
                }
                _ => Ok(Int(0)),
            }
        }
        IrBinOp::BitAnd | IrBinOp::BitOr | IrBinOp::BitXor | IrBinOp::Shl | IrBinOp::Shr => {
            match (a, b) {
                (IrValue::Int(x), IrValue::Int(y)) => {
                    let r = match op {
                        IrBinOp::BitAnd => x & y,
                        IrBinOp::BitOr => x | y,
                        IrBinOp::BitXor => x ^ y,
                        IrBinOp::Shl => x.wrapping_shl((*y % 128).max(0) as u32),
                        IrBinOp::Shr => x.wrapping_shr((*y % 128).max(0) as u32),
                        _ => 0,
                    };
                    Ok(IrValue::Int(r))
                }
                _ => Ok(IrValue::Int(0)),
            }
        }
        IrBinOp::Eq | IrBinOp::Ne | IrBinOp::Lt | IrBinOp::Le | IrBinOp::Gt | IrBinOp::Ge => {
            let r = match op {
                IrBinOp::Eq => a.value_eq(ctx, b),
                IrBinOp::Ne => !a.value_eq(ctx, b),
                IrBinOp::Lt => value_lt(a, b),
                IrBinOp::Le => value_lt(a, b) || a.value_eq(ctx, b),
                IrBinOp::Gt => !value_lt(a, b) && !a.value_eq(ctx, b),
                IrBinOp::Ge => !value_lt(a, b),
                _ => false,
            };
            Ok(IrValue::Bool(r))
        }
    }
}

pub(crate) fn value_lt(a: &IrValue, b: &IrValue) -> bool {
    match (a, b) {
        (IrValue::Int(x), IrValue::Int(y)) => x < y,
        (IrValue::Int(x), IrValue::Float(y)) => (*x as f64) < *y,
        (IrValue::Float(x), IrValue::Int(y)) => *x < *y as f64,
        (IrValue::Float(x), IrValue::Float(y)) => x < y,
        (IrValue::Str(x), IrValue::Str(y)) => x < y,
        (IrValue::Bool(x), IrValue::Bool(y)) => x < y,
        // 指针序：cell 索引序（稳定全序——对齐 tree-walking 按 Rc 地址序）
        (IrValue::Ptr(x), IrValue::Ptr(y)) => x < y,
        _ => false,
    }
}
