// stage2/src/lower.hc — S6：AST → IrInst（lower）
// 语义映射对照 tag1 hc/src/ir/lower_impl.rs 的 stage2 子集：
//   - And/Or 短路 → JumpIfNot/JumpIf + Const Bool（对齐 lower_expr Binary 臂）
//   - try → JumpIfErr + Return 错误值（对齐 Expr::Try 臂；子集无 defer 排空）
//   - if (opt) |v| → JumpIfNull + Unwrap 绑定（对齐 Stmt::If capture 臂）
//   - 方法调用 → CallMethod（运行时 {Type}.{method} 分派 + self 注入）
//   - 限定调用（io.print/alloc.init/Vec.init…）→ Call 限定名（运行时隐式环境路由）
//   - @intCast 首参类型名 → Const Str（对齐 is_type_arg_pos）
//   - 类方法 → "{Class}.{method}" 函数名登记（对齐 lower_decl Class 臂）
// 确定性：全部按声明序遍历；无 Map（平行 Vec + 线性查，R1/R5）；错误码 = ErrorLit
//   首现序（package 0）。
// 子集外构造（for/switch/closure/数组字面量/函数值/浮点…）一律登记诊断并回填 Void——
// 由 main 在 lower 后集中判定，绝不静默产出错误字节码。
// ============================================================

class LowerBind {
    name: &[u8],
    slot: i64,
}

class LowerLoopCtx {
    brk: i64,
    cont: i64,
}

class Lower {
    // 模块表（扁平平行 Vec；结束时经 lower_finish 组装 IrModule）
    funcs: Vec<IrFunc>,
    func_keys: Vec<&[u8]>,
    func_vals: Vec<i64>,
    globals: Vec<&[u8]>,
    err_names: Vec<&[u8]>,
    err_codes: Vec<i64>,
    enum_names: Vec<&[u8]>,
    enum_vars: Vec<Vec<&[u8]>>,
    // 当前函数构建区
    cur_name: &[u8],
    cur_params: Vec<i64>,
    cur_ptys: Vec<&[u8]>,
    cur_body: Vec<IrInst>,
    cur_n_slots: i64,
    cur_exported: bool,
    lbl_cnt: i64,
    // 作用域（checker 同款：扁平绑定表 + size 回滚）
    binds: Vec<LowerBind>,
    scope_sizes: Vec<i64>,
    // 循环栈（break/continue 目标）
    loops: Vec<LowerLoopCtx>,
    // 诊断（子集外构造）
    errs: Vec<&[u8]>,

    // ---- 方法（self: *mut Self；H 方法必须在类体内）----

    fn lo_err(self: *mut Self, msg: &[u8]) void {
        if (self.errs.len < 32) {
            self.errs.append(msg);
        }
    }

    fn lo_alloc_slot(self: *mut Self) i64 {
        var s = self.cur_n_slots;
        self.cur_n_slots = self.cur_n_slots + 1;
        return s;
    }

    fn lo_new_label(self: *mut Self) i64 {
        var id = self.lbl_cnt;
        self.lbl_cnt = self.lbl_cnt + 1;
        return id;
    }

    fn lo_push(self: *mut Self, x: IrInst) void {
        self.cur_body.append(x);
    }

    fn lo_push_scope(self: *mut Self) void {
        self.scope_sizes.append(@intCast(i64, self.binds.len));
    }

    fn lo_pop_scope(self: *mut Self) void {
        // Vec 无 pop；remove(len-1) 回滚（checker.hc 同款）
        var sz = self.scope_sizes[self.scope_sizes.len - 1];
        self.scope_sizes.remove(self.scope_sizes.len - 1);
        while (self.binds.len > @intCast(usize, sz)) {
            self.binds.remove(self.binds.len - 1);
        }
    }

    fn lo_bind(self: *mut Self, name: &[u8], slot: i64) void {
        var b = LowerBind{ name = name, slot = slot };
        self.binds.append(b);
    }

    fn lo_resolve(self: *mut Self, name: &[u8]) i64 {
        var mut i: i64 = @intCast(i64, self.binds.len) - 1;
        while (i >= 0) {
            var idx: usize = @intCast(usize, i);
            var b = self.binds[idx];
            if (ireq(b.name, name)) { return b.slot; }
            i -= 1;
        }
        return -1;
    }

    fn lo_is_global(self: *mut Self, name: &[u8]) bool {
        var mut i: usize = 0;
        while (i < self.globals.len) {
            if (ireq(self.globals[i], name)) { return true; }
            i += 1;
        }
        return false;
    }

    fn lo_func_idx(self: *mut Self, name: &[u8]) i64 {
        var mut i: usize = 0;
        while (i < self.func_keys.len) {
            if (ireq(self.func_keys[i], name)) { return self.func_vals[i]; }
            i += 1;
        }
        return -1;
    }

    fn lo_register_func(self: *mut Self, name: &[u8]) void {
        if (self.lo_func_idx(name) < 0) {
            self.func_keys.append(name);
            self.func_vals.append(-1);
        }
    }

    fn lo_err_code(self: *mut Self, name: &[u8]) i64 {
        var mut i: usize = 0;
        while (i < self.err_names.len) {
            if (ireq(self.err_names[i], name)) { return self.err_codes[i]; }
            i += 1;
        }
        return 0;
    }

    fn lo_walk_defs(self: *mut Self, n: AstNode) void {
        if (ireq(n.kind, "ErrorLit")) {
            if (get_prop(n.props, "name")) |en| {
                var mut known = false;
                var mut i: usize = 0;
                while (i < self.err_names.len) {
                    if (ireq(self.err_names[i], en)) { known = true; }
                    i += 1;
                }
                if (!known) {
                    var code: i64 = @intCast(i64, self.err_names.len);
                    self.err_codes.append(code);
                    self.err_names.append(en);
                }
            }
            return;
        }
        if (ireq(n.kind, "Enum")) {
            if (get_prop(n.props, "name")) |en| {
                var vars = Vec<&[u8]>.init(alloc);
                var mut i: usize = 0;
                while (i < n.children.len) {
                    var v = n.children[i];
                    if (ireq(v.kind, "Variant")) {
                        if (get_prop(v.props, "name")) |vn| {
                            vars.append(vn);
                        }
                    }
                    i += 1;
                }
                self.enum_names.append(en);
                self.enum_vars.append(vars);
            }
            return;
        }
        // 函数名注册（扁平 + {Class}.{method}；静态调用/内建遮蔽判定用）
        if (ireq(n.kind, "Fn")) {
            if (get_prop(n.props, "name")) |fn_name| {
                self.lo_register_func(fn_name);
            }
            return;
        }
        if (ireq(n.kind, "Class")) {
            if (get_prop(n.props, "name")) |cn| {
                var mut i: usize = 0;
                while (i < n.children.len) {
                    var m = n.children[i];
                    // stage2 parser：方法 = Fn 节点 + method=类名 prop（对齐 checker.hc）
                    if (ireq(m.kind, "Fn")) {
                        if (get_prop(m.props, "name")) |mn| {
                            self.lo_register_func(lo_join3(cn, ".", mn));
                        }
                    }
                    i += 1;
                }
            }
            // 类体内不再含错误字面量/枚举（方法体经 Fn 分支覆盖）
            return;
        }
        var mut i: usize = 0;
        while (i < n.children.len) {
            self.lo_walk_defs(n.children[i]);
            i += 1;
        }
    }

    fn lo_fn_begin(self: *mut Self, name: &[u8], exported: bool) void {
        self.cur_name = name;
        self.cur_params = Vec<i64>.init(alloc);
        self.cur_ptys = Vec<&[u8]>.init(alloc);
        self.cur_body = Vec<IrInst>.init(alloc);
        self.cur_n_slots = 0;
        self.cur_exported = exported;
        self.lbl_cnt = 0;
        self.binds = Vec<LowerBind>.init(alloc);
        self.scope_sizes = Vec<i64>.init(alloc);
    }

    fn lo_fn_end(self: *mut Self) void {
        // 兜底 ReturnVoid：坠穿函数由运行时 NoReturn 拒绝（对齐 exec_body 检查）
        self.lo_push(ir_return_void());
        var f = ir_func_new(self.cur_name);
        f.params = self.cur_params;
        f.param_tys = self.cur_ptys;
        f.n_slots = self.cur_n_slots;
        f.body = self.cur_body;
        f.exported = self.cur_exported;
        var idx: i64 = @intCast(i64, self.funcs.len);
        self.funcs.append(f);
        // 回填注册表占位（预收集 -1）或追加
        var mut done = false;
        var mut i: usize = 0;
        while (i < self.func_keys.len) {
            if (ireq(self.func_keys[i], self.cur_name)) {
                self.func_vals[i] = idx;
                done = true;
            }
            i += 1;
        }
        if (!done) {
            self.func_keys.append(self.cur_name);
            self.func_vals.append(idx);
        }
    }

    fn lo_params(self: *mut Self, owner: AstNode) void {
        var mut i: usize = 0;
        while (i < owner.children.len) {
            var p = owner.children[i];
            if (ireq(p.kind, "Param")) {
                var slot = self.lo_alloc_slot();
                if (get_prop(p.props, "name")) |pn| {
                    self.lo_bind(pn, slot);
                }
                var mut ty: &[u8] = "void";
                if (get_prop(p.props, "ty")) |t| { ty = t; }
                self.cur_params.append(slot);
                self.cur_ptys.append(ty);
            }
            i += 1;
        }
    }

    fn lo_program(self: *mut Self, prog: AstNode) void {
        var mut i: usize = 0;
        while (i < prog.children.len) {
            var d = prog.children[i];
            if (ireq(d.kind, "Fn")) {
                if (get_prop(d.props, "name")) |fn_name| {
                    var mut exported = false;
                    if (get_prop(d.props, "pub")) |pv| {
                        exported = ireq(pv, "true");
                    }
                    self.lo_fn_begin(fn_name, exported);
                    self.lo_params(d);
                    self.lo_fn_body(d);
                    self.lo_fn_end();
                }
            } else if (ireq(d.kind, "Class")) {
                if (get_prop(d.props, "name")) |cn| {
                    var j: usize = 0;
                    while (j < d.children.len) {
                        var m = d.children[j];
                        // 方法 = Fn + method=类名（对齐 stage2 parser/checker）
                        if (ireq(m.kind, "Fn")) {
                            if (get_prop(m.props, "name")) |mn| {
                                self.lo_fn_begin(lo_join3(cn, ".", mn), false);
                                self.lo_params(m);
                                self.lo_fn_body(m);
                                self.lo_fn_end();
                            }
                        }
                        j += 1;
                    }
                }
            } else if (ireq(d.kind, "Global") or ireq(d.kind, "Const") or ireq(d.kind, "Namespace")) {
                self.lo_err("lower: 子集外顶层声明");
            }
            // Enum/Import/UnknownDecl/Script/Comptime/Union/Interface：无运行时代码
            i += 1;
        }
    }

    fn lo_fn_body(self: *mut Self, owner: AstNode) void {
        var mut i: usize = 0;
        while (i < owner.children.len) {
            var c = owner.children[i];
            if (ireq(c.kind, "Block")) {
                self.lo_stmts(c);
                return;
            }
            i += 1;
        }
    }

    fn lo_stmts(self: *mut Self, b: AstNode) void {
        var mut i: usize = 0;
        while (i < b.children.len) {
            self.lo_stmt(b.children[i]);
            i += 1;
        }
    }

    fn lo_stmt(self: *mut Self, s: AstNode) void {
        if (ireq(s.kind, "VarDecl")) {
            var slot = self.lo_alloc_slot();
            if (get_prop(s.props, "name")) |vn| {
                self.lo_bind(vn, slot);
            }
            var mut t: i64 = 0;
            if (s.children.len > 0) {
                t = self.lo_expr(s.children[0]);
            } else {
                var mut ty: &[u8] = "";
                if (get_prop(s.props, "ty")) |tv| { ty = tv; }
                t = self.lo_default_value(ty);
            }
            self.lo_push(ir_store(slot, t));
            return;
        }
        if (ireq(s.kind, "ConstDecl")) {
            // 解析器未保留 init 子节点——本地 const 不在子集（stage2 源码未用）
            self.lo_err("lower: 本地 const 不在子集");
            return;
        }
        if (ireq(s.kind, "ExprStmt")) {
            if (s.children.len > 0) {
                var e = s.children[0];
                if (ireq(e.kind, "Assign")) {
                    self.lo_assign(e);
                } else {
                    self.lo_expr(e);
                }
            }
            return;
        }
        if (ireq(s.kind, "If")) {
            self.lo_if(s);
            return;
        }
        if (ireq(s.kind, "While")) {
            self.lo_while(s);
            return;
        }
        if (ireq(s.kind, "Return")) {
            if (s.children.len > 0) {
                var t = self.lo_expr(s.children[0]);
                self.lo_push(ir_return(t));
            } else {
                self.lo_push(ir_return_void());
            }
            return;
        }
        if (ireq(s.kind, "Break")) {
            if (self.loops.len > 0) {
                self.lo_push(ir_jump(self.loops[self.loops.len - 1].brk));
            } else {
                self.lo_err("lower: 循环外 break");
            }
            return;
        }
        if (ireq(s.kind, "Continue")) {
            if (self.loops.len > 0) {
                self.lo_push(ir_jump(self.loops[self.loops.len - 1].cont));
            } else {
                self.lo_err("lower: 循环外 continue");
            }
            return;
        }
        if (ireq(s.kind, "Block")) {
            self.lo_push_scope();
            self.lo_stmts(s);
            self.lo_pop_scope();
            return;
        }
        if (ireq(s.kind, "Empty")) {
            return;
        }
        if (ireq(s.kind, "For") or ireq(s.kind, "Switch") or ireq(s.kind, "Defer") or ireq(s.kind, "Errdefer")) {
            self.lo_err("lower: 子集外语句");
            return;
        }
        self.lo_err("lower: 未知语句");
    }

    fn lo_default_value(self: *mut Self, ty: &[u8]) i64 {
        var t = self.lo_alloc_slot();
        if (ireq(ty, "bool")) {
            self.lo_push(ir_const_inst(t, ir_const_bool(false)));
        } else if (ireq(ty, "i8") or ireq(ty, "i16") or ireq(ty, "i32") or ireq(ty, "i64")
            or ireq(ty, "u8") or ireq(ty, "u16") or ireq(ty, "u32") or ireq(ty, "u64")
            or ireq(ty, "usize") or ireq(ty, "isize")) {
            self.lo_push(ir_const_inst(t, ir_const_int(0)));
        } else {
            self.lo_err("lower: 子集外无初值声明类型");
            self.lo_push(ir_const_inst(t, ir_const_void()));
        }
        return t;
    }

    fn lo_if(self: *mut Self, s: AstNode) void {
        var l_else = self.lo_new_label();
        var l_end = self.lo_new_label();
        var mut payload: ?&[u8] = null;
        if (get_prop(s.props, "payload")) |pv| { payload = pv; }
        var mut payload_err: ?&[u8] = null;
        if (get_prop(s.props, "payload_err")) |pe| { payload_err = pe; }
        var has_else = s.children.len > 2;
        var c = self.lo_expr(s.children[0]);
        var mut l_err: i64 = -1;
        if (payload_err != null) {
            if (payload != null) {
                l_err = self.lo_new_label();
                self.lo_push(ir_jump_if_err(c, l_err));
            }
        }
        if (payload != null) {
            // 可选捕获：null → else；否则解包绑定（对齐 Rust Stmt::If capture 臂）
            self.lo_push(ir_jump_if_null(c, l_else));
            self.lo_push_scope();
            var u = self.lo_alloc_slot();
            self.lo_push(ir_unwrap(u, c));
            self.lo_bind(payload.?, u);
            self.lo_stmts(s.children[1]);
            self.lo_pop_scope();
        } else {
            self.lo_push(ir_jump_if_not(c, l_else));
            self.lo_push_scope();
            self.lo_stmts(s.children[1]);
            self.lo_pop_scope();
        }
        if (has_else) {
            self.lo_push(ir_jump(l_end));
            self.lo_push(ir_label(l_else));
            if (l_err >= 0) {
                // null 非错误路径不进 else（对齐 Rust err_capture 语义）
                self.lo_push(ir_jump(l_end));
                self.lo_push(ir_label(l_err));
                self.lo_push_scope();
                self.lo_bind(payload_err.?, c);
                self.lo_stmt_or_if(s.children[2]);
                self.lo_pop_scope();
            } else {
                self.lo_stmt_or_if(s.children[2]);
            }
        } else {
            self.lo_push(ir_label(l_else));
            if (l_err >= 0) {
                self.lo_push(ir_label(l_err));
            }
        }
        self.lo_push(ir_label(l_end));
    }

    fn lo_stmt_or_if(self: *mut Self, n: AstNode) void {
        if (ireq(n.kind, "If")) {
            self.lo_if(n);
        } else {
            self.lo_stmts(n);
        }
    }

    fn lo_while(self: *mut Self, s: AstNode) void {
        var l_top = self.lo_new_label();
        var l_cont = self.lo_new_label();
        var l_end = self.lo_new_label();
        var mut payload: ?&[u8] = null;
        if (get_prop(s.props, "payload")) |pv| { payload = pv; }
        var mut payload_err: ?&[u8] = null;
        if (get_prop(s.props, "payload_err")) |pe| { payload_err = pe; }
        var mut l_err: i64 = -1;
        if (payload_err != null) {
            if (payload != null) {
                l_err = self.lo_new_label();
            }
        }
        self.lo_push(ir_label(l_top));
        var c = self.lo_expr(s.children[0]);
        if (payload != null) {
            if (l_err >= 0) {
                // 错误沿调用链上浮：return 错误值（对齐 Rust exec_while 错误传播）
                self.lo_push(ir_jump_if_err(c, l_err));
            }
            self.lo_push(ir_jump_if_null(c, l_end));
            self.lo_push_scope();
            var u = self.lo_alloc_slot();
            self.lo_push(ir_unwrap(u, c));
            self.lo_bind(payload.?, u);
            var lp = LowerLoopCtx{ brk = l_end, cont = l_cont };
            self.loops.append(lp);
            self.lo_stmts(s.children[1]);
            self.loops.remove(self.loops.len - 1);
            self.lo_pop_scope();
        } else {
            self.lo_push(ir_jump_if_not(c, l_end));
            var lp = LowerLoopCtx{ brk = l_end, cont = l_cont };
            self.loops.append(lp);
            self.lo_stmts(s.children[1]);
            self.loops.remove(self.loops.len - 1);
        }
        self.lo_push(ir_label(l_cont));
        self.lo_push(ir_jump(l_top));
        if (l_err >= 0) {
            self.lo_push(ir_label(l_err));
            self.lo_push(ir_return(c));
        }
        self.lo_push(ir_label(l_end));
    }

    fn lo_assign(self: *mut Self, a: AstNode) void {
        var mut op: &[u8] = "Eq";
        if (get_prop(a.props, "op")) |ov| { op = ov; }
        var target = a.children[0];
        var value = a.children[1];
        var mut binop: &[u8] = "";
        if (ireq(op, "PlusEq")) { binop = "Add"; }
        else if (ireq(op, "MinusEq")) { binop = "Sub"; }
        else if (ireq(op, "StarEq")) { binop = "Mul"; }
        else if (ireq(op, "SlashEq")) { binop = "Div"; }
        if (ireq(target.kind, "Ident")) {
            var mut tn: &[u8] = "";
            if (get_prop(target.props, "name")) |nv| { tn = nv; }
            var slot = self.lo_resolve(tn);
            if (slot < 0) {
                self.lo_err("lower: 赋值目标未解析");
                return;
            }
            var v = self.lo_expr(value);
            if (binop.len > 0) {
                var cur = self.lo_alloc_slot();
                self.lo_push(ir_load(cur, slot));
                var nv = self.lo_alloc_slot();
                self.lo_push(ir_bin(binop, nv, cur, v));
                self.lo_push(ir_store(slot, nv));
            } else {
                self.lo_push(ir_store(slot, v));
            }
            return;
        }
        if (ireq(target.kind, "Field")) {
            var mut fname: &[u8] = "";
            if (get_prop(target.props, "field")) |fv| { fname = fv; }
            var b = self.lo_expr(target.children[0]);
            var v = self.lo_expr(value);
            if (binop.len > 0) {
                var cur = self.lo_alloc_slot();
                self.lo_push(ir_field(cur, b, fname));
                var nv = self.lo_alloc_slot();
                self.lo_push(ir_bin(binop, nv, cur, v));
                self.lo_push(ir_store_field(b, fname, nv));
            } else {
                self.lo_push(ir_store_field(b, fname, v));
            }
            return;
        }
        if (ireq(target.kind, "Index")) {
            // b[i] = v（复合 b[i] op= v = 读 cur + binop + 写回；单索引）
            var b = self.lo_expr(target.children[0]);
            var ix = self.lo_expr(target.children[1]);
            var v = self.lo_expr(value);
            if (binop.len > 0) {
                var cur = self.lo_alloc_slot();
                self.lo_push(ir_index(cur, b, ix));
                var nv = self.lo_alloc_slot();
                self.lo_push(ir_bin(binop, nv, cur, v));
                self.lo_push(ir_store_index(b, ix, nv));
            } else {
                self.lo_push(ir_store_index(b, ix, v));
            }
            return;
        }
        self.lo_err("lower: 子集外赋值目标");
    }

    fn lo_expr(self: *mut Self, e: AstNode) i64 {
        var t = self.lo_alloc_slot();
        if (ireq(e.kind, "IntLit")) {
            var mut txt: &[u8] = "0";
            if (get_prop(e.props, "text")) |tv| { txt = tv; }
            self.lo_push(ir_const_inst(t, ir_const_int(lower_parse_int(txt))));
            return t;
        }
        if (ireq(e.kind, "StrLit")) {
            var mut v: &[u8] = "";
            if (get_prop(e.props, "value")) |vv| { v = vv; }
            self.lo_push(ir_const_inst(t, ir_const_str(v)));
            return t;
        }
        if (ireq(e.kind, "CharLit")) {
            // 值以十进制文本存储（S2 修复）
            var mut v: &[u8] = "0";
            if (get_prop(e.props, "value")) |vv| { v = vv; }
            self.lo_push(ir_const_inst(t, ir_const_int(lower_parse_int(v))));
            return t;
        }
        if (ireq(e.kind, "BoolLit")) {
            var mut bv = false;
            if (get_prop(e.props, "value")) |vv| { bv = ireq(vv, "true"); }
            self.lo_push(ir_const_inst(t, ir_const_bool(bv)));
            return t;
        }
        if (ireq(e.kind, "NullLit")) {
            self.lo_push(ir_const_inst(t, ir_const_null()));
            return t;
        }
        if (ireq(e.kind, "VoidLit")) {
            self.lo_push(ir_const_inst(t, ir_const_void()));
            return t;
        }
        if (ireq(e.kind, "ErrorLit")) {
            var mut en: &[u8] = "";
            if (get_prop(e.props, "name")) |nv| { en = nv; }
            self.lo_push(ir_const_inst(t, ir_const_err(en, self.lo_err_code(en))));
            return t;
        }
        if (ireq(e.kind, "FloatLit")) {
            var mut txt: &[u8] = "0.0";
            if (get_prop(e.props, "text")) |tv| { txt = tv; }
            self.lo_push(ir_const_inst(t, ir_const_float(lower_parse_float(txt))));
            return t;
        }
        if (ireq(e.kind, "Ident")) {
            var mut n: &[u8] = "";
            if (get_prop(e.props, "name")) |nv| { n = nv; }
            var slot = self.lo_resolve(n);
            if (slot >= 0) {
                self.lo_push(ir_load(t, slot));
            } else if (self.lo_is_global(n)) {
                self.lo_push(ir_load_global(t, n));
            } else {
                self.lo_err("lower: 未知标识符");
                self.lo_push(ir_const_inst(t, ir_const_void()));
            }
            return t;
        }
        if (ireq(e.kind, "Field")) {
            // 类字段/内建属性加载（.len/.props…）；指针基座由运行时自动解引用
            var b = self.lo_expr(e.children[0]);
            var mut fname: &[u8] = "";
            if (get_prop(e.props, "field")) |fv| { fname = fv; }
            self.lo_push(ir_field(t, b, fname));
            return t;
        }
        if (ireq(e.kind, "Index")) {
            var b = self.lo_expr(e.children[0]);
            var idx = e.children[1];
            var mut idx_op: &[u8] = "";
            if (ireq(idx.kind, "Binary")) {
                if (get_prop(idx.props, "op")) |ov| { idx_op = ov; }
            }
            if (ireq(idx_op, "Range")) {
                // 切片 b[lo..hi]（两端点由源码显式给出）
                var lo = self.lo_expr(idx.children[0]);
                var hi = self.lo_expr(idx.children[1]);
                self.lo_push(ir_slice_of(t, b, lo, hi));
            } else {
                var i = self.lo_expr(idx);
                self.lo_push(ir_index(t, b, i));
            }
            return t;
        }
        if (ireq(e.kind, "Binary")) {
            var mut op: &[u8] = "";
            if (get_prop(e.props, "op")) |ov| { op = ov; }
            if (ireq(op, "And") or ireq(op, "Or")) {
                var a = self.lo_expr(e.children[0]);
                var l_short = self.lo_new_label();
                var done = self.lo_new_label();
                if (ireq(op, "And")) {
                    self.lo_push(ir_jump_if_not(a, l_short));
                } else {
                    self.lo_push(ir_jump_if(a, l_short));
                }
                var b = self.lo_expr(e.children[1]);
                self.lo_push(ir_load(t, b));
                self.lo_push(ir_jump(done));
                self.lo_push(ir_label(l_short));
                self.lo_push(ir_const_inst(t, ir_const_bool(ireq(op, "Or"))));
                self.lo_push(ir_label(done));
            } else if (ireq(op, "Range")) {
                self.lo_err("lower: 独立区间不在子集");
                self.lo_push(ir_const_inst(t, ir_const_void()));
            } else {
                var a = self.lo_expr(e.children[0]);
                var b = self.lo_expr(e.children[1]);
                self.lo_push(ir_bin(op, t, a, b));
            }
            return t;
        }
        if (ireq(e.kind, "Unary")) {
            var mut op: &[u8] = "";
            if (get_prop(e.props, "op")) |ov| { op = ov; }
            var a = self.lo_expr(e.children[0]);
            self.lo_push(ir_un(op, t, a));
            return t;
        }
        if (ireq(e.kind, "AddrOf")) {
            var target = e.children[0];
            if (ireq(target.kind, "Ident")) {
                var mut n: &[u8] = "";
                if (get_prop(target.props, "name")) |nv| { n = nv; }
                var slot = self.lo_resolve(n);
                if (slot >= 0) {
                    self.lo_push(ir_addr_slot(t, slot));
                    return t;
                }
            }
            self.lo_err("lower: 非变量取址不在子集");
            self.lo_push(ir_const_inst(t, ir_const_void()));
            return t;
        }
        if (ireq(e.kind, "Deref")) {
            var a = self.lo_expr(e.children[0]);
            self.lo_push(ir_deref(t, a));
            return t;
        }
        if (ireq(e.kind, "Unwrap")) {
            var a = self.lo_expr(e.children[0]);
            self.lo_push(ir_unwrap(t, a));
            return t;
        }
        if (ireq(e.kind, "Try")) {
            // try：错误值从当前函数返回（值通道；对齐 Expr::Try 臂，无 defer 排空）
            var a = self.lo_expr(e.children[0]);
            var l_ret = self.lo_new_label();
            var done = self.lo_new_label();
            self.lo_push(ir_jump_if_err(a, l_ret));
            self.lo_push(ir_load(t, a));
            self.lo_push(ir_jump(done));
            self.lo_push(ir_label(l_ret));
            self.lo_push(ir_return(a));
            self.lo_push(ir_label(done));
            return t;
        }
        if (ireq(e.kind, "Catch")) {
            // catch：错误值 → 处理分支；结果统一到 res_slot（对齐 tag1 lower_expr Catch 臂）
            //   Bind 形式（catch |err| { body }）：err 值绑定进作用域；body 需以 return 结尾
            //   （块值形式未支持——子集切片）；Default 形式（catch 默认值）：错误 → 默认表达式
            var a = self.lo_expr(e.children[0]);
            var l_catch = self.lo_new_label();
            var done = self.lo_new_label();
            var res_slot = self.lo_alloc_slot();
            self.lo_push(ir_jump_if_err(a, l_catch));
            self.lo_push(ir_store(res_slot, a));
            self.lo_push(ir_jump(done));
            self.lo_push(ir_label(l_catch));
            var c1 = e.children[1];
            if (ireq(c1.kind, "Bind")) {
                var mut nm: &[u8] = "";
                if (get_prop(c1.props, "name")) |nv| { nm = nv; }
                var err_slot = self.lo_alloc_slot();
                self.lo_push(ir_store(err_slot, a));
                self.lo_push_scope();
                self.lo_bind(nm, err_slot);
                self.lo_stmts(c1.children[0]);
                self.lo_pop_scope();
            } else {
                var h = self.lo_expr(c1.children[0]);
                self.lo_push(ir_store(res_slot, h));
            }
            self.lo_push(ir_label(done));
            self.lo_push(ir_load(t, res_slot));
            return t;
        }
        if (ireq(e.kind, "Call")) {
            return self.lo_call(e, t);
        }
        if (ireq(e.kind, "ClassLit")) {
            var mut ty: &[u8] = "";
            if (get_prop(e.props, "name")) |nv| { ty = nv; }
            var names = Vec<&[u8]>.init(alloc);
            var vals = Vec<i64>.init(alloc);
            var mut i: usize = 0;
            while (i < e.children.len) {
                var fi = e.children[i];
                if (ireq(fi.kind, "FieldInit")) {
                    var mut fname: &[u8] = "";
                    if (get_prop(fi.props, "name")) |nv| { fname = nv; }
                    names.append(fname);
                    if (fi.children.len == 0) {
                        self.lo_err("lower: 字段缺省初始化不在子集");
                        var vt = self.lo_alloc_slot();
                        self.lo_push(ir_const_inst(vt, ir_const_void()));
                        vals.append(vt);
                    } else {
                        vals.append(self.lo_expr(fi.children[0]));
                    }
                }
                i += 1;
            }
            self.lo_push(ir_make_class(t, ty, names, vals));
            return t;
        }
        if (ireq(e.kind, "Move")) {
            // move X：值拷贝转移（对齐 tag1 lower_impl Expr::Move 臂 + IrInst::Move opcode 29）
            var a = self.lo_expr(e.children[0]);
            self.lo_push(ir_move(t, a));
            return t;
        }
        if (ireq(e.kind, "Orelse") or ireq(e.kind, "Await") or ireq(e.kind, "Closure") or ireq(e.kind, "Dot")
            or ireq(e.kind, "ArrayLit") or ireq(e.kind, "TupleLit") or ireq(e.kind, "NamedLit")
            or ireq(e.kind, "IfExpr") or ireq(e.kind, "SwitchExpr") or ireq(e.kind, "Unknown")
            or ireq(e.kind, "StructType") or ireq(e.kind, "ContainerLit")) {
            lo_err_kind(self, "lower: 子集外表达式 ", e.kind);
            self.lo_push(ir_const_inst(t, ir_const_void()));
            return t;
        }
        lo_err_kind(self, "lower: 未知表达式 ", e.kind);
        self.lo_push(ir_const_inst(t, ir_const_void()));
        return t;
    }

    fn lo_call(self: *mut Self, node: AstNode, t: i64) i64 {
        var callee = node.children[0];
        if (ireq(callee.kind, "Ident")) {
            var mut n: &[u8] = "";
            if (get_prop(callee.props, "name")) |nv| { n = nv; }
            var mut is_builtin = false;
            if (n.len > 0) {
                if (n[0] == '@') { is_builtin = true; }
            }
            // 还原无 @ 的内建名（stage2 lexer 的 AtBuiltin token 文本不含 @；CallBuiltin 运行时名带 @）
            var bname = n;
            if (!is_builtin) {
                if (lower_is_at_builtin(n)) {
                    bname = lo_join3("@", "", n);
                    is_builtin = true;
                }
            }
            if (!is_builtin) {
                if (lower_is_free_builtin(n) and self.lo_func_idx(n) < 0) { is_builtin = true; }
            }
            var args = self.lo_args(node, bname);
            if (is_builtin) {
                self.lo_push(ir_call_builtin(bname, args, t));
            } else if (self.lo_resolve(n) >= 0) {
                // 函数值调用（CallIndirect）不在子集
                self.lo_err("lower: 函数值调用不在子集");
                self.lo_push(ir_const_inst(t, ir_const_void()));
            } else {
                self.lo_push(ir_call(n, args, t));
            }
            return t;
        }
        if (ireq(callee.kind, "Field")) {
            var mut method: &[u8] = "";
            if (get_prop(callee.props, "field")) |fv| { method = fv; }
            var qn = lo_qualified_name(callee);
            // 限定静态调用：根 Ident 不解析为局部（io.print/Vec.init/alloc.init…）；
            // 已注册限定名（命名空间函数/{Type}.{method} 静态形）优先（对齐 Rust lower 顺序）
            if (qn.len > 0) {
                var root = lo_root_name(callee);
                if (self.lo_func_idx(qn) >= 0) {
                    var args = self.lo_args(node, qn);
                    self.lo_push(ir_call(qn, args, t));
                    return t;
                }
                if (self.lo_resolve(root) < 0) {
                    var args = self.lo_args(node, qn);
                    self.lo_push(ir_call(qn, args, t));
                    return t;
                }
            }
            // 实例方法：base 求值 + 运行时 {Type}.{method} 分派（self 注入首参）
            var b = self.lo_expr(callee.children[0]);
            var args = self.lo_args(node, "");
            self.lo_push(ir_call_method(t, b, method, args));
            return t;
        }
        self.lo_err("lower: 子集外调用形态");
        self.lo_push(ir_const_inst(t, ir_const_void()));
        return t;
    }

    fn lo_args(self: *mut Self, node: AstNode, callee_name: &[u8]) Vec<i64> {
        var args = Vec<i64>.init(alloc);
        var mut i: usize = 1;
        while (i < node.children.len) {
            var a = node.children[i];
            var pos = i - 1;
            if (lower_is_type_arg_pos(callee_name, pos) and ireq(a.kind, "Ident")) {
                var mut tn: &[u8] = "";
                if (get_prop(a.props, "name")) |nv| { tn = nv; }
                var at = self.lo_alloc_slot();
                self.lo_push(ir_const_inst(at, ir_const_str(tn)));
                args.append(at);
            } else {
                args.append(self.lo_expr(a));
            }
            i += 1;
        }
        return args;
    }
}

// 带节点种类的诊断（动态拼接；append_bytes 来自 checker.hc 同命名空间）
fn lo_err_kind(l: *mut Lower, msg: &[u8], kind: &[u8]) void {
    if (l.errs.len < 32) {
        var buf = Vec<u8>.init(alloc);
        append_bytes(&buf, msg);
        append_bytes(&buf, kind);
        l.errs.append(buf.as_slice());
    }
}

fn lower_new() Lower {
    return Lower{
        funcs = Vec<IrFunc>.init(alloc),
        func_keys = Vec<&[u8]>.init(alloc),
        func_vals = Vec<i64>.init(alloc),
        globals = ir_implicit_env_names(),
        err_names = Vec<&[u8]>.init(alloc),
        err_codes = Vec<i64>.init(alloc),
        enum_names = Vec<&[u8]>.init(alloc),
        enum_vars = Vec<Vec<&[u8]>>.init(alloc),
        cur_name = "",
        cur_params = Vec<i64>.init(alloc),
        cur_ptys = Vec<&[u8]>.init(alloc),
        cur_body = Vec<IrInst>.init(alloc),
        cur_n_slots = 0,
        cur_exported = false,
        lbl_cnt = 0,
        binds = Vec<LowerBind>.init(alloc),
        scope_sizes = Vec<i64>.init(alloc),
        loops = Vec<LowerLoopCtx>.init(alloc),
        errs = Vec<&[u8]>.init(alloc),
    };
}

fn lower_is_free_builtin(name: &[u8]) bool {
    if (ireq(name, "box") or ireq(name, "unbox") or ireq(name, "copy")) { return true; }
    if (ireq(name, "sqrt") or ireq(name, "min") or ireq(name, "max")) { return true; }
    if (ireq(name, "fmt_int") or ireq(name, "fmt_float")) { return true; }
    if (ireq(name, "read_u64_le")) { return true; }
    if (ireq(name, "sort") or ireq(name, "binary_search")) { return true; }
    if (ireq(name, "skip_space") or ireq(name, "peek") or ireq(name, "advance")) { return true; }
    if (ireq(name, "is_digit") or ireq(name, "parse_number")) { return true; }
    if (ireq(name, "parse_int") or ireq(name, "parse_float")) { return true; }
    if (ireq(name, "spawn")) { return true; }
    if (ireq(name, "Pipe") or ireq(name, "Tee") or ireq(name, "Funnel") or ireq(name, "Hub")) { return true; }
    return false;
}

// @ 内建名（stage2 lexer 的 AtBuiltin token 文本不含 @；parser 折叠为 Ident 调用）
fn lower_is_at_builtin(name: &[u8]) bool {
    if (ireq(name, "intCast") or ireq(name, "floatCast")) { return true; }
    if (ireq(name, "sizeOf") or ireq(name, "alignOf") or ireq(name, "offsetOf")) { return true; }
    if (ireq(name, "enumFromInt") or ireq(name, "ptrCast") or ireq(name, "alignCast")) { return true; }
    if (ireq(name, "atomicLoad") or ireq(name, "atomicStore") or ireq(name, "atomicRmw")) { return true; }
    if (ireq(name, "bitCast") or ireq(name, "intFromPtr") or ireq(name, "ptrFromInt")) { return true; }
    return false;
}

fn lower_is_type_arg_pos(name: &[u8], pos: usize) bool {
    if (ireq(name, "@sizeOf") or ireq(name, "@alignOf")) { return pos == 0; }
    if (ireq(name, "@offsetOf")) { return pos == 0 or pos == 1; }
    if (ireq(name, "@intCast") or ireq(name, "@enumFromInt") or ireq(name, "@ptrCast") or ireq(name, "@alignCast")) { return pos == 0; }
    if (ireq(name, "@atomicLoad") or ireq(name, "@atomicStore") or ireq(name, "@atomicRmw")) { return pos == 0; }
    if (ireq(name, "alloc.init") or ireq(name, "arena.init")) { return pos == 0; }
    return false;
}

// 十进制浮点文本 → f64（纯十进制切片，不含指数/下划线；逐位合成与 stage1 parse_float_text 一致）
fn lower_parse_float(text: &[u8]) f64 {
    var mut i: usize = 0;
    var mut neg = false;
    if (i < text.len and text[i] == '-') { neg = true; i += 1; }
    var mut v: f64 = 0.0;
    while (i < text.len and text[i] >= '0' and text[i] <= '9') {
        v = v * 10.0 + @intCast(f64, text[i] - '0');
        i += 1;
    }
    if (i < text.len and text[i] == '.') {
        i += 1;
        var mut scale: f64 = 0.1;
        while (i < text.len and text[i] >= '0' and text[i] <= '9') {
            v = v + @intCast(f64, text[i] - '0') * scale;
            scale = scale * 0.1;
            i += 1;
        }
    }
    if (neg) { v = -v; }
    return v;
}

fn lower_parse_int(text: &[u8]) i64 {
    var clean = Vec<u8>.init(alloc);
    var mut i: usize = 0;
    while (i < text.len) {
        var c = text[i];
        var is_dig = c >= '0' and c <= '9';
        var is_hex = (c >= 'a' and c <= 'f') or (c >= 'A' and c <= 'F');
        var is_us = c == '_';
        var is_pfx = c == 'x' or c == 'X' or c == 'b' or c == 'B' or c == 'o' or c == 'O';
        if (is_dig or is_hex or is_us or is_pfx) {
            clean.append(c);
        } else {
            // 对齐 take_while：首遇非法字符即止
            i = text.len;
        }
        i += 1;
    }
    // 去下划线
    var digits = Vec<u8>.init(alloc);
    i = 0;
    while (i < clean.len) {
        if (clean[i] != '_') { digits.append(clean[i]); }
        i += 1;
    }
    var mut radix: i64 = 10;
    var mut start: usize = 0;
    if (digits.len > 1 and digits[0] == '0' and (digits[1] == 'x' or digits[1] == 'X')) {
        radix = 16;
        start = 2;
    } else if (digits.len > 1 and digits[0] == '0' and (digits[1] == 'b' or digits[1] == 'B')) {
        radix = 2;
        start = 2;
    } else if (digits.len > 1 and digits[0] == '0' and (digits[1] == 'o' or digits[1] == 'O')) {
        radix = 8;
        start = 2;
    }
    var mut v: i64 = 0;
    var mut ovf = false;
    i = start;
    while (i < digits.len) {
        var mut d: i64 = -1;
        var c = digits[i];
        if (c >= '0' and c <= '9') { d = @intCast(i64, c - '0'); }
        else if (c >= 'a' and c <= 'f') { d = @intCast(i64, c - 'a') + 10; }
        else if (c >= 'A' and c <= 'F') { d = @intCast(i64, c - 'A') + 10; }
        if (d < 0 or d >= radix) { return 0; }
        v = v * radix + d;
        if (v < 0) { ovf = true; }
        i += 1;
    }
    if (ovf) { return 0; }
    return v;
}

// 限定名展平：Field 链 + 根 Ident → "io.fs.read_file"；非此形返回 ""（实例方法路径）。
// AstNode 为引用类型不可重绑 → 递归下降收集段名
fn lo_qual_parts(e: AstNode, parts: *mut Vec<&[u8]>) bool {
    if (ireq(e.kind, "Field")) {
        var mut fname: &[u8] = "";
        if (get_prop(e.props, "field")) |fv| { fname = fv; }
        parts.*.append(fname);
        if (e.children.len == 0) { return false; }
        return lo_qual_parts(e.children[0], parts);
    }
    if (ireq(e.kind, "Ident")) {
        var mut rn: &[u8] = "";
        if (get_prop(e.props, "name")) |nv| { rn = nv; }
        parts.*.append(rn);
        return true;
    }
    return false;
}

fn lo_qualified_name(e: AstNode) &[u8] {
    var parts = Vec<&[u8]>.init(alloc);
    if (!lo_qual_parts(e, &parts)) { return ""; }
    // 逆序拼接
    var out = Vec<u8>.init(alloc);
    var mut i: usize = parts.len;
    while (i > 0) {
        var seg = parts[i - 1];
        var mut j: usize = 0;
        while (j < seg.len) {
            out.append(seg[j]);
            j += 1;
        }
        if (i > 1) {
            out.append('.');
        }
        i -= 1;
    }
    return out.as_slice();
}

// Field 链根名（io.fs.read_file → io；根非 Ident → ""）
fn lo_root_name(e: AstNode) &[u8] {
    if (ireq(e.kind, "Field")) {
        if (e.children.len == 0) { return ""; }
        return lo_root_name(e.children[0]);
    }
    if (ireq(e.kind, "Ident")) {
        var mut rn: &[u8] = "";
        if (get_prop(e.props, "name")) |nv| { rn = nv; }
        return rn;
    }
    return "";
}

fn lo_join3(a: &[u8], sep: &[u8], b: &[u8]) &[u8] {
    var out = Vec<u8>.init(alloc);
    var mut i: usize = 0;
    while (i < a.len) { out.append(a[i]); i += 1; }
    i = 0;
    while (i < sep.len) { out.append(sep[i]); i += 1; }
    i = 0;
    while (i < b.len) { out.append(b[i]); i += 1; }
    return out.as_slice();
}

fn lower_module(prog: AstNode) Lower {
    var l = lower_new();
    l.lo_walk_defs(prog);
    l.lo_program(prog);
    return l;
}

fn lower_finish(l: *mut Lower) IrModule {
    var m = ir_module_new();
    m.funcs = l.funcs;
    m.func_keys = l.func_keys;
    m.func_vals = l.func_vals;
    m.globals = l.globals;
    m.err_names = l.err_names;
    m.err_codes = l.err_codes;
    m.enum_names = l.enum_names;
    m.enum_vars = l.enum_vars;
    return m;
}
