// stage2/src/ir.hc — S5：IR 模型（IrModule / IrFunc / IrInst / IrConst）
// 对照 tag1 hc/src/ir/models/ir_inst.rs（49 变体）圈定 stage2 子集指令集（24 变体）：
//   Const/Load/Store/Bin/Un/Jump/JumpIf/JumpIfNot/JumpIfNull/Label/
//   Call/CallBuiltin/JumpIfErr/Return/ReturnVoid/AddrSlot/Deref/Field/StoreField/
//   Index/StoreIndex/SliceOf/MakeClass/Unwrap/CallMethod/LoadGlobal（25 变体；StoreIndex
//   为编码器排序表等自身代码所需）
// 不入子集（stage2 自身源码未用，遇之响亮报错）：StorePtr/StoreIndex/MakeArr/
//   MakeEnum/Move/MakeRange/FnRef/CallIndirect/LoadGlobal 写/迭代器/闭包/defer 系/DeepCopy。
// 建模约定（对齐 stage2 纪律 R1/R7）：class + kind 字符串分发；字段扁平；
//   Map 只存标量（R2）；需要顺序处一律平行 Vec（R1）。
// ============================================================
// 指令字段语义（扁平复用，构造器为准）：
//   temp = 目的 temp（JumpIf/JumpIfNot/JumpIfNull/JumpIfErr = 条件 temp）
//   a    = 操作数 1（Store=slot；CallMethod=base；AddrSlot=slot；Deref/Unwrap=src）
//   b    = 操作数 2（Index=index；SliceOf=lo）
//   c    = label（Jump 系）/ SliceOf=hi
//   op   = Bin/Un 运算名（"Add"…/"Neg"/"Not"/"BitNot"）
//   name = Call 名 / Field 字段名 / MakeClass 类名 / LoadGlobal 全局名
//   args = Call/CallMethod 实参 temp 表 / MakeClass 值 temp 表
//   fields = MakeClass 字段名表（与 args 平行）
//   konst = Const 载荷
// ============================================================

class IrConst {
    kind: &[u8],   // Int / Float / Bool / Str / Void / Null / Err / End
    i: i64,
    f: f64,
    b: bool,
    s: &[u8],
    name: &[u8],   // Err 错误名
}

fn ir_const_int(v: i64) IrConst {
    return IrConst{ kind = "Int", i = v, f = 0.0, b = false, s = "", name = "" };
}

fn ir_const_float(v: f64) IrConst {
    return IrConst{ kind = "Float", i = 0, f = v, b = false, s = "", name = "" };
}

fn ir_const_bool(v: bool) IrConst {
    return IrConst{ kind = "Bool", i = 0, f = 0.0, b = v, s = "", name = "" };
}

fn ir_const_str(s: &[u8]) IrConst {
    return IrConst{ kind = "Str", i = 0, f = 0.0, b = false, s = s, name = "" };
}

fn ir_const_void() IrConst {
    return IrConst{ kind = "Void", i = 0, f = 0.0, b = false, s = "", name = "" };
}

fn ir_const_null() IrConst {
    return IrConst{ kind = "Null", i = 0, f = 0.0, b = false, s = "", name = "" };
}

fn ir_const_err(name: &[u8], code: i64) IrConst {
    var c = IrConst{ kind = "Err", i = code, f = 0.0, b = false, s = "", name = name };
    return c;
}

class IrInst {
    kind: &[u8],
    temp: i64,
    a: i64,
    b: i64,
    c: i64,
    op: &[u8],
    name: &[u8],
    args: Vec<i64>,
    fields: Vec<&[u8]>,
    konst: IrConst,
}

fn ir_inst(kind: &[u8]) IrInst {
    return IrInst{
        kind = kind,
        temp = 0,
        a = 0,
        b = 0,
        c = 0,
        op = "",
        name = "",
        args = Vec<i64>.init(alloc),
        fields = Vec<&[u8]>.init(alloc),
        konst = ir_const_void(),
    };
}

// ---- 指令构造器（字段语义见文件头）----

fn ir_const_inst(temp: i64, val: IrConst) IrInst {
    var x = ir_inst("Const");
    x.temp = temp;
    x.konst = val;
    return x;
}

fn ir_load(temp: i64, slot: i64) IrInst {
    var x = ir_inst("Load");
    x.temp = temp;
    x.a = slot;
    return x;
}

fn ir_store(slot: i64, temp: i64) IrInst {
    var x = ir_inst("Store");
    x.a = slot;
    x.temp = temp;
    return x;
}

fn ir_bin(op: &[u8], temp: i64, a: i64, b: i64) IrInst {
    var x = ir_inst("Bin");
    x.op = op;
    x.temp = temp;
    x.a = a;
    x.b = b;
    return x;
}

fn ir_un(op: &[u8], temp: i64, a: i64) IrInst {
    var x = ir_inst("Un");
    x.op = op;
    x.temp = temp;
    x.a = a;
    return x;
}

fn ir_jump(label: i64) IrInst {
    var x = ir_inst("Jump");
    x.c = label;
    return x;
}

fn ir_jump_if(temp: i64, label: i64) IrInst {
    var x = ir_inst("JumpIf");
    x.temp = temp;
    x.c = label;
    return x;
}

fn ir_jump_if_not(temp: i64, label: i64) IrInst {
    var x = ir_inst("JumpIfNot");
    x.temp = temp;
    x.c = label;
    return x;
}

fn ir_jump_if_null(temp: i64, label: i64) IrInst {
    var x = ir_inst("JumpIfNull");
    x.temp = temp;
    x.c = label;
    return x;
}

fn ir_label(id: i64) IrInst {
    var x = ir_inst("Label");
    x.c = id;
    return x;
}

fn ir_call(name: &[u8], args: Vec<i64>, temp: i64) IrInst {
    var x = ir_inst("Call");
    x.name = name;
    x.args = args;
    x.temp = temp;
    return x;
}

fn ir_call_builtin(name: &[u8], args: Vec<i64>, temp: i64) IrInst {
    var x = ir_inst("CallBuiltin");
    x.name = name;
    x.args = args;
    x.temp = temp;
    return x;
}

fn ir_jump_if_err(temp: i64, label: i64) IrInst {
    var x = ir_inst("JumpIfErr");
    x.temp = temp;
    x.c = label;
    return x;
}

fn ir_return(temp: i64) IrInst {
    var x = ir_inst("Return");
    x.temp = temp;
    return x;
}

fn ir_return_void() IrInst {
    return ir_inst("ReturnVoid");
}

fn ir_addr_slot(temp: i64, slot: i64) IrInst {
    var x = ir_inst("AddrSlot");
    x.temp = temp;
    x.a = slot;
    return x;
}

fn ir_deref(temp: i64, a: i64) IrInst {
    var x = ir_inst("Deref");
    x.temp = temp;
    x.a = a;
    return x;
}

fn ir_field(temp: i64, base: i64, name: &[u8]) IrInst {
    var x = ir_inst("Field");
    x.temp = temp;
    x.a = base;
    x.name = name;
    return x;
}

fn ir_store_field(base: i64, name: &[u8], value: i64) IrInst {
    var x = ir_inst("StoreField");
    x.a = base;
    x.name = name;
    x.temp = value;
    return x;
}

fn ir_index(temp: i64, base: i64, index: i64) IrInst {
    var x = ir_inst("Index");
    x.temp = temp;
    x.a = base;
    x.b = index;
    return x;
}

fn ir_store_index(base: i64, index: i64, value: i64) IrInst {
    var x = ir_inst("StoreIndex");
    x.a = base;
    x.b = index;
    x.temp = value;
    return x;
}

fn ir_slice_of(temp: i64, base: i64, lo: i64, hi: i64) IrInst {
    var x = ir_inst("SliceOf");
    x.temp = temp;
    x.a = base;
    x.b = lo;
    x.c = hi;
    return x;
}

fn ir_make_class(temp: i64, ty: &[u8], names: Vec<&[u8]>, vals: Vec<i64>) IrInst {
    var x = ir_inst("MakeClass");
    x.temp = temp;
    x.name = ty;
    x.fields = names;
    x.args = vals;
    return x;
}

fn ir_unwrap(temp: i64, a: i64) IrInst {
    var x = ir_inst("Unwrap");
    x.temp = temp;
    x.a = a;
    return x;
}

fn ir_call_method(temp: i64, base: i64, method: &[u8], args: Vec<i64>) IrInst {
    var x = ir_inst("CallMethod");
    x.temp = temp;
    x.a = base;
    x.name = method;
    x.args = args;
    return x;
}

fn ir_load_global(temp: i64, name: &[u8]) IrInst {
    var x = ir_inst("LoadGlobal");
    x.temp = temp;
    x.name = name;
    return x;
}

class IrFunc {
    name: &[u8],
    params: Vec<i64>,
    param_tys: Vec<&[u8]>,
    n_slots: i64,
    body: Vec<IrInst>,
    exported: bool,
}

fn ir_func_new(name: &[u8]) IrFunc {
    return IrFunc{
        name = name,
        params = Vec<i64>.init(alloc),
        param_tys = Vec<&[u8]>.init(alloc),
        n_slots = 0,
        body = Vec<IrInst>.init(alloc),
        exported = false,
    };
}

// 模块表（S7 编码输入）；全部平行 Vec（R1：禁 Map 迭代/R5：避免 Map.put 语义陷阱）
class IrModule {
    funcs: Vec<IrFunc>,
    func_keys: Vec<&[u8]>,         // 名 → 下标（与 func_vals 平行；一名一函数）
    func_vals: Vec<i64>,
    globals: Vec<&[u8]>,           // 隐式环境名 + 用户全局（运行时 init 预分配 cell）
    err_names: Vec<&[u8]>,         // 平行 err_codes（首现序 = 码）
    err_codes: Vec<i64>,
    enum_names: Vec<&[u8]>,        // 平行 enum_vars
    enum_vars: Vec<Vec<&[u8]>>,    // 枚举名 → 变体名序
}

fn ir_module_new() IrModule {
    return IrModule{
        funcs = Vec<IrFunc>.init(alloc),
        func_keys = Vec<&[u8]>.init(alloc),
        func_vals = Vec<i64>.init(alloc),
        globals = Vec<&[u8]>.init(alloc),
        err_names = Vec<&[u8]>.init(alloc),
        err_codes = Vec<i64>.init(alloc),
        enum_names = Vec<&[u8]>.init(alloc),
        enum_vars = Vec<Vec<&[u8]>>.init(alloc),
    };
}

// 隐式环境名（对齐 tag1 ir/runtime.rs IMPLICIT_ENV；LoadGlobal 落点 + 运行时预置值）
fn ir_implicit_env_names() Vec<&[u8]> {
    var v = Vec<&[u8]>.init(alloc);
    v.append("alloc");
    v.append("io");
    v.append("test_io");
    v.append("stdout");
    v.append("stderr");
    v.append("pi");
    v.append("Vec");
    v.append("Deque");
    v.append("Map");
    v.append("Table");
    return v;
}

// 字节级切片相等（本文件自带，避免跨文件符号顺序耦合）
fn ireq(a: &[u8], b: &[u8]) bool {
    if (a.len != b.len) { return false; }
    var mut i: usize = 0;
    while (i < a.len) {
        if (a[i] != b[i]) { return false; }
        i += 1;
    }
    return true;
}

// 模块函数表查询（线性；未注册 → -1）
fn ir_module_func_idx(m: IrModule, name: &[u8]) i64 {
    var mut i: usize = 0;
    while (i < m.func_keys.len) {
        if (ireq(m.func_keys[i], name)) { return m.func_vals[i]; }
        i += 1;
    }
    return -1;
}
