// stage2/src/encode.hc — S7：HBC2 编码器（IrModule → 字节序列）
// 逐字段对照 tag1 hc/src/codegen/bytecode/{encode.rs,decode.rs,opcode.rs}；
// 魔数 "HBC2"、版本 7（v7 = H1 增 union 表后的现行版本，本子集 unions/continuous 恒空表）。
// 编码确定性：func_index/错误码/枚举表按名字节序排序（对齐 encode.rs sort_by）；
// 其余表（funcs/globals/枚举变体序）按声明序输出。
// 读回契约：decode.rs 丢 ret_ty/type_implements 无碍（execute_ir 不依赖）。
// ============================================================

fn enc_push_u32(out: *mut Vec<u8>, v: i64) void {
    // v 视为 u32 位型（0..2^32-1）；i64 算术移位 + 掩码逐字节取出
    out.*.append(@intCast(u8, v & 0xFF));
    out.*.append(@intCast(u8, (v >> 8) & 0xFF));
    out.*.append(@intCast(u8, (v >> 16) & 0xFF));
    out.*.append(@intCast(u8, (v >> 24) & 0xFF));
}

fn enc_push_str(out: *mut Vec<u8>, s: &[u8]) void {
    enc_push_u32(out, @intCast(i64, s.len));
    var mut i: usize = 0;
    while (i < s.len) {
        out.*.append(s[i]);
        i += 1;
    }
}

// IrConst::Int 在 HBC2 中为 i128 小端 16 字节——i64 符号扩展到 128 位。
// 算术移位 + 掩码 = 补码字节型（负数高位字节为 0xFF），与 i128 小端一致
fn enc_push_i128_le(out: *mut Vec<u8>, v: i64) void {
    var mut i: i64 = 0;
    while (i < 8) {
        var sh: i64 = i * 8;
        out.*.append(@intCast(u8, (v >> sh) & 0xFF));
        i += 1;
    }
    // 高 64 位 = 符号位填充（负数全 0xFF，非负全 0x00）
    var mut fill: i64 = 0;
    if (v < 0) { fill = 0xFF; }
    i = 0;
    while (i < 8) {
        out.*.append(@intCast(u8, fill));
        i += 1;
    }
}

// ---- 常量 / 运算符标签（对齐 opcode.rs）----

fn enc_const_tag(kind: &[u8]) i64 {
    if (ireq(kind, "Int")) { return 0; }
    if (ireq(kind, "Bool")) { return 2; }
    if (ireq(kind, "Str")) { return 3; }
    if (ireq(kind, "Void")) { return 4; }
    if (ireq(kind, "Null")) { return 5; }
    if (ireq(kind, "Err")) { return 6; }
    if (ireq(kind, "End")) { return 7; }
    return 255; // 不可达：lower 不产 Float/End 之外的未知 kind
}

fn enc_binop_tag(op: &[u8]) i64 {
    if (ireq(op, "Add")) { return 0; }
    if (ireq(op, "Sub")) { return 1; }
    if (ireq(op, "Mul")) { return 2; }
    if (ireq(op, "Div")) { return 3; }
    if (ireq(op, "Mod")) { return 4; }
    if (ireq(op, "EucMod") or ireq(op, "ModMod")) { return 5; }
    if (ireq(op, "BitAnd")) { return 6; }
    if (ireq(op, "BitOr")) { return 7; }
    if (ireq(op, "BitXor")) { return 8; }
    if (ireq(op, "Shl")) { return 9; }
    if (ireq(op, "Shr")) { return 10; }
    if (ireq(op, "Eq")) { return 11; }
    if (ireq(op, "Ne")) { return 12; }
    if (ireq(op, "Lt")) { return 13; }
    if (ireq(op, "Le")) { return 14; }
    if (ireq(op, "Gt")) { return 15; }
    if (ireq(op, "Ge")) { return 16; }
    return 255;
}

fn enc_unop_tag(op: &[u8]) i64 {
    if (ireq(op, "Neg")) { return 0; }
    if (ireq(op, "Not")) { return 1; }
    if (ireq(op, "BitNot")) { return 2; }
    return 255;
}

fn enc_const(out: *mut Vec<u8>, c: IrConst) void {
    out.*.append(@intCast(u8, enc_const_tag(c.kind)));
    if (ireq(c.kind, "Int")) {
        enc_push_i128_le(out, c.i);
    } else if (ireq(c.kind, "Bool")) {
        var mut byte: i64 = 0;
        if (c.b) { byte = 1; }
        out.*.append(@intCast(u8, byte));
    } else if (ireq(c.kind, "Str")) {
        enc_push_str(out, c.s);
    } else if (ireq(c.kind, "Err")) {
        enc_push_str(out, c.name);
        enc_push_u32(out, c.i);
    }
    // Void/Null/End 仅 tag 字节
}

// ---- 指令 / 函数 / 类型 ----

fn enc_inst(out: *mut Vec<u8>, x: IrInst) void {
    if (ireq(x.kind, "Const")) {
        out.*.append(0);
        enc_push_u32(out, x.temp);
        enc_const(out, x.konst);
    } else if (ireq(x.kind, "Load")) {
        out.*.append(1);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
    } else if (ireq(x.kind, "Store")) {
        out.*.append(2);
        enc_push_u32(out, x.a);
        enc_push_u32(out, x.temp);
    } else if (ireq(x.kind, "Bin")) {
        out.*.append(3);
        out.*.append(@intCast(u8, enc_binop_tag(x.op)));
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
        enc_push_u32(out, x.b);
    } else if (ireq(x.kind, "Un")) {
        out.*.append(4);
        out.*.append(@intCast(u8, enc_unop_tag(x.op)));
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
    } else if (ireq(x.kind, "Jump")) {
        out.*.append(5);
        enc_push_u32(out, x.c);
    } else if (ireq(x.kind, "JumpIf")) {
        out.*.append(6);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.c);
    } else if (ireq(x.kind, "JumpIfNot")) {
        out.*.append(7);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.c);
    } else if (ireq(x.kind, "JumpIfNull")) {
        out.*.append(8);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.c);
    } else if (ireq(x.kind, "Label")) {
        out.*.append(9);
        enc_push_u32(out, x.c);
    } else if (ireq(x.kind, "Call")) {
        out.*.append(10);
        enc_push_str(out, x.name);
        enc_push_u32(out, @intCast(i64, x.args.len));
        var mut i: usize = 0;
        while (i < x.args.len) {
            enc_push_u32(out, x.args[i]);
            i += 1;
        }
        enc_push_u32(out, x.temp);
    } else if (ireq(x.kind, "CallBuiltin")) {
        out.*.append(11);
        enc_push_str(out, x.name);
        enc_push_u32(out, @intCast(i64, x.args.len));
        var mut i: usize = 0;
        while (i < x.args.len) {
            enc_push_u32(out, x.args[i]);
            i += 1;
        }
        enc_push_u32(out, x.temp);
    } else if (ireq(x.kind, "JumpIfErr")) {
        out.*.append(12);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.c);
    } else if (ireq(x.kind, "Return")) {
        out.*.append(13);
        enc_push_u32(out, x.temp);
    } else if (ireq(x.kind, "ReturnVoid")) {
        out.*.append(14);
    } else if (ireq(x.kind, "AddrSlot")) {
        out.*.append(15);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
    } else if (ireq(x.kind, "Deref")) {
        out.*.append(17);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
    } else if (ireq(x.kind, "Field")) {
        out.*.append(19);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
        enc_push_str(out, x.name);
    } else if (ireq(x.kind, "StoreField")) {
        out.*.append(20);
        enc_push_u32(out, x.a);
        enc_push_str(out, x.name);
        enc_push_u32(out, x.temp);
    } else if (ireq(x.kind, "Index")) {
        out.*.append(21);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
        enc_push_u32(out, x.b);
    } else if (ireq(x.kind, "StoreIndex")) {
        out.*.append(22);
        enc_push_u32(out, x.a);
        enc_push_u32(out, x.b);
        enc_push_u32(out, x.temp);
    } else if (ireq(x.kind, "SliceOf")) {
        out.*.append(23);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
        enc_push_u32(out, x.b);
        enc_push_u32(out, x.c);
    } else if (ireq(x.kind, "MakeClass")) {
        out.*.append(26);
        enc_push_u32(out, x.temp);
        enc_push_str(out, x.name);
        enc_push_u32(out, @intCast(i64, x.args.len));
        var mut i: usize = 0;
        while (i < x.args.len) {
            enc_push_str(out, x.fields[i]);
            enc_push_u32(out, x.args[i]);
            i += 1;
        }
    } else if (ireq(x.kind, "Unwrap")) {
        out.*.append(30);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
    } else if (ireq(x.kind, "CallMethod")) {
        out.*.append(40);
        enc_push_u32(out, x.temp);
        enc_push_u32(out, x.a);
        enc_push_str(out, x.name);
        enc_push_u32(out, @intCast(i64, x.args.len));
        var mut i: usize = 0;
        while (i < x.args.len) {
            enc_push_u32(out, x.args[i]);
            i += 1;
        }
    } else if (ireq(x.kind, "LoadGlobal")) {
        out.*.append(41);
        enc_push_u32(out, x.temp);
        enc_push_str(out, x.name);
    } else {
        // 不可达（lower 只发子集指令）；发 255 使 decode 响亮失败而非静默损坏
        out.*.append(255);
    }
}

// Type 编码：子集仅 Named（tag 0，无泛型实参）——param_ty 运行时不用（decode 亦丢 ret_ty）
fn enc_type_named(out: *mut Vec<u8>, name: &[u8]) void {
    out.*.append(0);
    enc_push_str(out, name);
    enc_push_u32(out, 0);
}

fn enc_func(out: *mut Vec<u8>, f: IrFunc) void {
    enc_push_str(out, f.name);
    enc_push_u32(out, @intCast(i64, f.params.len));
    var mut i: usize = 0;
    while (i < f.params.len) {
        enc_push_u32(out, f.params[i]);
        i += 1;
    }
    enc_push_u32(out, @intCast(i64, f.param_tys.len));
    i = 0;
    while (i < f.param_tys.len) {
        enc_type_named(out, f.param_tys[i]);
        i += 1;
    }
    // param_defaults（与 params 等长全 false）+ defaults（恒空）
    enc_push_u32(out, @intCast(i64, f.params.len));
    i = 0;
    while (i < f.params.len) {
        out.*.append(0);
        i += 1;
    }
    enc_push_u32(out, 0);
    enc_push_u32(out, f.n_slots);
    out.*.append(0); // is_test
    var mut ex: i64 = 0;
    if (f.exported) { ex = 1; }
    out.*.append(@intCast(u8, ex));
    enc_push_u32(out, @intCast(i64, f.body.len));
    i = 0;
    while (i < f.body.len) {
        enc_inst(out, f.body[i]);
        i += 1;
    }
}

// ---- 名字排序（字节字典序，插入排序；对齐 encode.rs sort_by 名比较）----

fn enc_name_lt(a: &[u8], b: &[u8]) bool {
    var mut n: usize = a.len;
    if (b.len < n) { n = b.len; }
    var mut i: usize = 0;
    while (i < n) {
        if (a[i] != b[i]) { return a[i] < b[i]; }
        i += 1;
    }
    return a.len < b.len;
}

// 对 idxs（平行 keys 的下标表）按 keys 名字典序升序插入排序（值进值出，避免指针下标写）
fn enc_sort_by_name(keys: Vec<&[u8]>, idxs: Vec<i64>) Vec<i64> {
    var mut v = copy(&idxs);
    var mut i: usize = 1;
    while (i < v.len) {
        var mut j: usize = i;
        while (j > 0) {
            var pj: usize = j - 1;
            if (enc_name_lt(keys[@intCast(usize, v[j])], keys[@intCast(usize, v[pj])])) {
                var t = v[j];
                v[j] = v[pj];
                v[pj] = t;
                j -= 1;
            } else { break; }
        }
        i += 1;
    }
    return v;
}

// ---- 模块编码（字段序 = decode.rs 读回序，不可调换）----

fn enc_module(m: IrModule) Vec<u8> {
    var out = Vec<u8>.init(alloc);
    out.*.append('H');
    out.*.append('B');
    out.*.append('C');
    out.*.append('2');
    enc_push_u32(&out, 7);
    // funcs 数 + func_index（按名排序；一名一候选；func_keys/func_vals 平行表）
    enc_push_u32(&mut out, @intCast(i64, m.funcs.len));
    var fnames = Vec<&[u8]>.init(alloc);
    var order = Vec<i64>.init(alloc);
    var mut i: usize = 0;
    while (i < m.func_keys.len) {
        fnames.append(m.func_keys[i]);
        order.append(@intCast(i64, i));
        i += 1;
    }
    var mut order2 = enc_sort_by_name(fnames, order);
    enc_push_u32(&mut out, @intCast(i64, order2.len));
    i = 0;
    while (i < order2.len) {
        var o0 = order2[i];
        var ki: usize = @intCast(usize, o0);
        enc_push_str(&out, fnames[ki]);
        enc_push_u32(&out, 1);
        enc_push_u32(&out, m.func_vals[ki]);
        i += 1;
    }
    i = 0;
    while (i < m.funcs.len) {
        var mut mk = Vec<u8>.init(alloc);
        append_int(i, &mut mk);
        append_bytes(&mk, ":");
        append_bytes(&mk, m.funcs[i].name);
        append_bytes(&mk, " slots=");
        append_int(m.funcs[i].n_slots, &mut mk);
        append_bytes(&mk, " body=");
        append_int(@intCast(i64, m.funcs[i].body.len), &mut mk);
        try io.fs.write_file("stage2/test/dbg_enc.txt", mk.as_slice(), alloc);
        enc_func(&out, m.funcs[i]);
        i += 1;
    }
    // closures（恒空）
    enc_push_u32(&mut out, 0);
    // globals（声明/固定序）
    enc_push_u32(&mut out, @intCast(i64, m.globals.len));
    i = 0;
    while (i < m.globals.len) {
        enc_push_str(&out, m.globals[i]);
        i += 1;
    }
    // error_codes（按名排序）
    var enames = Vec<&[u8]>.init(alloc);
    var eorder = Vec<i64>.init(alloc);
    i = 0;
    while (i < m.err_names.len) {
        enames.append(m.err_names[i]);
        eorder.append(@intCast(i64, i));
        i += 1;
    }
    var mut eorder2 = enc_sort_by_name(enames, eorder);
    enc_push_u32(&mut out, @intCast(i64, eorder2.len));
    i = 0;
    while (i < eorder2.len) {
        var ei: usize = @intCast(usize, eorder2[i]);
        enc_push_str(&out, enames[ei]);
        enc_push_u32(&out, m.err_codes[ei]);
        i += 1;
    }
    // enum_variants（枚举名排序；变体保持声明序）
    var vnames = Vec<&[u8]>.init(alloc);
    var vorder = Vec<i64>.init(alloc);
    i = 0;
    while (i < m.enum_names.len) {
        vnames.append(m.enum_names[i]);
        vorder.append(@intCast(i64, i));
        i += 1;
    }
    var mut vorder2 = enc_sort_by_name(vnames, vorder);
    enc_push_u32(&mut out, @intCast(i64, vorder2.len));
    i = 0;
    while (i < vorder2.len) {
        var vi: usize = @intCast(usize, vorder2[i]);
        enc_push_str(&out, vnames[vi]);
        var vars = m.enum_vars[vi];
        enc_push_u32(&mut out, @intCast(i64, vars.len));
        var mut j: usize = 0;
        while (j < vars.len) {
            enc_push_str(&out, vars[j]);
            j += 1;
        }
        i += 1;
    }
    // continuous / unions（本子集恒空表）
    enc_push_u32(&mut out, 0);
    enc_push_u32(&mut out, 0);
    return out;
}
