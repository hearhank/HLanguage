// stage2/src/checker.hc — 阶段 3：语义检查（S4 裁剪版）
// 从 stage1/checker.hc 提取：仅保留 Checker 类与类型/签名/调用点检查；
// 所有权机制（moved/AllocSource/ADR-0030）与错误集推断已切除；
// 工具性副本（Lexer/Parser/main）已删——AST 与助手经同命名空间扁平共享
// 来自 src/lexer.hc、src/parser.hc（ADR-0031）。

// 诊断消息字节追加（checker 独有）
fn append_bytes(msg: *mut Vec<u8>, s: &[u8]) void {
    var mut i: usize = 0;
    while (i < s.len) {
        msg.*.append(s[i]);
        i += 1;
    }
}



// ============================================================
// 核心类型系统
// ============================================================

// 整数宽度
enum IntWidth {
    I8,
    I16,
    I32,
    I64,
    I128,
    ISize,
    U8,
    U16,
    U32,
    U64,
    U128,
    USize,
    Comptime,
}

// 类型种类（简化版：用 &[u8] 标记类型名，运行时用 kind 字符串匹配）
// 具体类型：int|float|bool|void|str|named|ptr|slice|optional|error_union|tuple|array|infer|generic|unknown
class SType {
    kind: Vec<u8>,
    // 命名类型
    type_name: Vec<u8>,
    type_args: Vec<SType>,
    // 指针类型
    pointee: ?SType,
    ptr_mut: bool,
    // 切片类型
    elem_type: ?SType,
    // 数组类型
    array_len: i64,
    // 错误联合类型
    error_set_type: ?SType,
    inner_type: ?SType,
    // 元组类型
    elem_types: Vec<SType>,
}

// 创建基本类型
fn make_ty(kind: &[u8]) SType {
    return SType{
        kind = vec_from_slice(kind),
        type_name = Vec<u8>.init(alloc),
        type_args = Vec<SType>.init(alloc),
        pointee = null,
        ptr_mut = false,
        elem_type = null,
        array_len = 0,
        error_set_type = null,
        inner_type = null,
        elem_types = Vec<SType>.init(alloc),
    };
}


// 变量信息
class VarInfo {
    ty: SType,
    mut_: bool,
}

// 函数签名
class FnSig {
    param_types: Vec<SType>,
    ret_type: SType,
}

// ============================================================
// 语义检查器（Checker）
// ============================================================

// 作用域条目
class ScopeEntry {
    name: &[u8],
    info: VarInfo,
}

// 检查器状态
class Checker {
    // 诊断信息（错误消息列表）
    diags: Vec<Vec<u8>>,
    // 源码（用于行号定位）
    src: Vec<u8>,
    // 行号表（从源码构建）
    line_starts: Vec<usize>,
    // 作用域条目（扁平存储，从后向前查找）
    scopes: Vec<ScopeEntry>,
    // 每个作用域边界（push 时的 scopes.len）
    scope_sizes: Vec<usize>,
    // 类型注册表（名字→类型信息）
    types: Map<&[u8], SType>,
    // 函数注册表（名字→函数签名）
    funcs: Map<&[u8], FnSig>,
    // 当前函数是否声明了错误联合返回类型
    current_fn_ret_is_error_union: bool,
    // 当前正在检查的类名（空串 = 不在类内；用于 self 注册与 Self 解析）
    mut current_class: &[u8],

    // 初始化：从源码构建行号表
    fn init(self: *mut Self, src: Vec<u8>) void {
        self.src = src;
        self.line_starts.append(0);
        var mut i: usize = 0;
        while (i < src.len) {
            if (src[i] == '\n') {
                self.line_starts.append(i + 1);
            }
            i += 1;
        }
        self.push_scope();
    }

    // 添加错误
    fn error(self: *mut Self, msg: Vec<u8>) void {
        self.diags.append(msg);
    }

    // 推入新作用域
    fn push_scope(self: *mut Self) void {
        self.scope_sizes.append(self.scopes.len);
    }

    // 弹出作用域
    fn pop_scope(self: *mut Self) void {
        if (self.scope_sizes.len > 0) {
            var mut target = self.scope_sizes[self.scope_sizes.len - 1];
            self.scope_sizes.remove(self.scope_sizes.len - 1);
            while (self.scopes.len > target) {
                var entry = self.scopes[self.scopes.len - 1];
                self.scopes.remove(self.scopes.len - 1);
            }
        }
    }

    // 在当前作用域注册名字
    fn register(self: *mut Self, name: &[u8], info: VarInfo) void {
        var entry = ScopeEntry{name = name, info = info};
        self.scopes.append(entry);
    }

    // 从当前作用域栈查找名字（从最内层向外查找）
    fn lookup(self: *mut Self, name: &[u8]) ?VarInfo {
        var mut i: i64 = @intCast(i64, self.scopes.len) - 1;
        while (i >= 0) {
            var entry = self.scopes[@intCast(usize, i)];
            if (entry.name == name) {
                return entry.info;
            }
            i -= 1;
        }
        return null;
    }

    // 注册类型
    fn register_type(self: *mut Self, name: &[u8], ty: SType) void {
        self.types.put(name, ty);
    }

    // 查找类型
    fn lookup_type(self: *mut Self, name: &[u8]) ?SType {
        if (self.types.contains(name)) {
            return self.types.get(name);
        }
        return null;
    }

    // 注册函数
    fn register_func(self: *mut Self, name: &[u8], sig: FnSig) void {
        self.funcs.put(name, sig);
    }

    // 查找函数
    fn lookup_func(self: *mut Self, name: &[u8]) ?FnSig {
        if (self.funcs.contains(name)) {
            return self.funcs.get(name);
        }
        return null;
    }

    // 类型解析：将类型名字符串转换为 SType
    fn ty_of(self: *mut Self, name: &[u8]) SType {
        // 内建整数类型
        if (name == "i8") return make_ty("i8");
        if (name == "i16") return make_ty("i16");
        if (name == "i32") return make_ty("i32");
        if (name == "i64") return make_ty("i64");
        if (name == "i128") return make_ty("i128");
        if (name == "isize") return make_ty("isize");
        if (name == "u8") return make_ty("u8");
        if (name == "u16") return make_ty("u16");
        if (name == "u32") return make_ty("u32");
        if (name == "u64") return make_ty("u64");
        if (name == "u128") return make_ty("u128");
        if (name == "usize") return make_ty("usize");
        if (name == "comptime_int") return make_ty("comptime_int");
        // 内建浮点类型
        if (name == "f16" or name == "f32" or name == "f64" or name == "f128") return make_ty("float");
        // 内建其他类型
        if (name == "bool") return make_ty("bool");
        if (name == "void") return make_ty("void");
        if (name == "String") return make_ty("str");
        if (name == "type" or name == "anytype") return make_ty("type");
        // 内建集合类型
        if (name == "Vec" or name == "Deque" or name == "Map" or name == "Table") return make_ty(name);
        if (name == "Allocator" or name == "ExitType") return make_ty(name);
        if (name == "Future") return make_ty(name);
        // 在类型注册表中查找
        if (self.types.contains(name)) {
            var t = self.types.get(name);
            if (t) |tt| { return tt; }
        }
        // 大写开头 → 泛型参数
        if (name.len > 0 and name[0] >= 'A' and name[0] <= 'Z') {
            return make_ty("generic");
        }
        // 未知类型
        return make_ty("unknown");
    }

    // 从 props 中解析类型注解
    fn resolve_ty(self: *mut Self, props: &[u8]) SType {
        var ty_prop = get_prop(props, "ty");
        if (ty_prop) |t| {
            return self.ty_of(t);
        }
        return make_ty("unknown");
    }

    // 从子节点中解析类型注解
    fn resolve_ty_children(self: *mut Self, children: Vec<AstNode>) SType {
        var mut ci: usize = 0;
        while (ci < children.len) {
            var child = children[ci];
            if (child.kind == "TypeName") {
                var tn = get_prop(child.props, "name");
                if (tn) |t| {
                    return self.ty_of(t);
                }
                break;
            }
            ci += 1;
        }
        return make_ty("unknown");
    }

    // 类型兼容性检查：值类型是否兼容于期望类型
    fn is_compatible(self: *mut Self, val_ty: SType, expect_ty: SType) bool {
        var vk = val_ty.kind.as_slice();
        var ek = expect_ty.kind.as_slice();
        // comptime_int 兼容任何整数类型
        if (vk == "comptime_int") {
            if (ek == "i8" or ek == "i16" or
                ek == "i32" or ek == "i64" or
                ek == "i128" or ek == "isize" or
                ek == "u8" or ek == "u16" or
                ek == "u32" or ek == "u64" or
                ek == "u128" or ek == "usize" or
                ek == "comptime_int") return true;
        }
        // 相同类型
        if (vk == ek) return true;
        return false;
    }

    // 推断分配来源

    // 获取表达式类型
    fn type_of_expr(self: *mut Self, expr: AstNode) SType {
        var k = expr.kind;
        if (k == "IntLit") { return make_ty("comptime_int"); }
        if (k == "FloatLit") { return make_ty("float"); }
        if (k == "BoolLit") { return make_ty("bool"); }
        if (k == "StrLit") { return make_ty("str"); }
        if (k == "CharLit") { return make_ty("u8"); }
        if (k == "NullLit") { return make_ty("null"); }
        if (k == "VoidLit") { return make_ty("void"); }
        if (k == "Ident") {
            var name = get_prop(expr.props, "name");
            if (name) |n| {
                // 在作用域中查找变量类型
                var found = self.lookup(n);
                if (found) |info| { return info.ty; }
                // 在类型注册表中查找
                if (self.types.contains(n)) {
                    var t = self.types.get(n);
                    if (t) |tt| { return tt; }
                }
                // 函数名 → 函数类型
                if (self.funcs.contains(n)) { return make_ty("fn"); }
                // 内建名称
                if (self.is_builtin_name(n)) {
                    if (n == "true" or n == "false") return make_ty("bool");
                    if (n == "null") return make_ty("null");
                    if (n == "void") return make_ty("void");
                    // 其他内建名（alloc, io 等）→ unknown
                }
            }
            return make_ty("unknown");
        }
        if (k == "Binary") {
            if (expr.children.len >= 3) {
                var op = expr.children[2].kind;
                var l = self.type_of_expr(expr.children[0]);
                var r = self.type_of_expr(expr.children[1]);
                // 逻辑运算符返回 bool
                if (op == "And" or op == "Or") {
                    return make_ty("bool");
                }
                // 比较运算符返回 bool
                if (op == "Eq" or op == "Ne" or op == "Lt" or
                    op == "Le" or op == "Gt" or op == "Ge") {
                    return make_ty("bool");
                }
                // 算术运算符：如果任一操作数是 float，结果 float
                if (l.kind.as_slice() == "float" or r.kind.as_slice() == "float") return make_ty("float");
                // 否则 return comptime_int（后续会收窄）
                return make_ty("comptime_int");
            }
            return make_ty("unknown");
        }
        if (k == "Unary") {
            if (expr.children.len > 0) {
                return self.type_of_expr(expr.children[0]);
            }
            return make_ty("unknown");
        }
        if (k == "Call") {
            // 检查是否是函数调用
            if (expr.children.len > 0) {
                var callee = expr.children[0];
                if (callee.kind == "Ident") {
                    var name = get_prop(callee.props, "name");
                    if (name) |n| {
                        // 在函数注册表中查找
                        if (self.funcs.contains(n)) {
                            var sig = self.funcs.get(n);
                            if (sig) |s| { return s.ret_type; }
                        }
                        // 内建函数（stage2 lexer：AtBuiltin token 文本不含 @）
                        if (n == "expect" or n == "expect_eq") return make_ty("void");
                        if (n == "@intCast" or n == "@floatCast") return make_ty("unknown");
                        if (n == "intCast" or n == "floatCast") return make_ty("unknown");
                        if (n.len > 0 and n[0] == '@') return make_ty("unknown");
                    }
                }
            }
            return make_ty("unknown");
        }
        if (k == "ArrayLit") { return make_ty("array"); }
        if (k == "AtBuiltin") { return make_ty("unknown"); }
        if (k == "Field") {
            // error.NotFound → 错误类型
            if (expr.children.len > 0) {
                var base = expr.children[0];
                if (base.kind == "Ident") {
                    var name = get_prop(base.props, "name");
                    if (name) |n| {
                        if (slice_eq(n, "error")) {
                            return make_ty("error_type");
                        }
                    }
                }
            }
            return make_ty("unknown");
        }
        if (k == "Index") { return make_ty("unknown"); }
        if (k == "Unwrap") {
            if (expr.children.len > 0) {
                return self.type_of_expr(expr.children[0]);
            }
            return make_ty("unknown");
        }
        return make_ty("unknown");
    }

    // 检查程序（两遍：收集 + 检查）
    fn check_program(self: *mut Self, prog: AstNode) void {
        // 第一遍：收集所有声明
        io.print("[check] collect: {} decls
", prog.children.len);
        self.collect_program(prog);
        // 第二遍：检查（每 10 个 decl 打心跳，嵌套解释下本阶段为小时级）
        var mut i: usize = 0;
        while (i < prog.children.len) {
            self.check_decl(prog.children[i]);
            i += 1;
            if (i % 10 == 0) {
                io.print("[check] {}/{} decls
", i, prog.children.len);
            }
        }
        io.print("[check] all {} decls checked
", prog.children.len);
    }

    // ========== 收集阶段（第一遍） ==========

    // 收集所有声明
    fn collect_program(self: *mut Self, prog: AstNode) void {
        var mut i: usize = 0;
        while (i < prog.children.len) {
            self.collect_decl(prog.children[i]);
            i += 1;
        }
    }

    // 收集单个声明
    fn collect_decl(self: *mut Self, decl: AstNode) void {
        var k = decl.kind;
        if (k == "Class") { self.collect_class(decl); }
        else if (k == "Enum") { self.collect_enum(decl); }
        else if (k == "Union") { self.collect_union(decl); }
        else if (k == "Interface") { self.collect_interface(decl); }
        else if (k == "Fn") { self.collect_fn(decl); }
        else if (k == "Namespace") {
            var mut i: usize = 0;
            while (i < decl.children.len) {
                self.collect_decl(decl.children[i]);
                i += 1;
            }
        }
    }

    // 收集 class 声明
    fn collect_class(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var ty = make_ty(n);
            self.register_type(n, ty);
        }
    }

    // 收集 enum 声明
    fn collect_enum(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var ty = make_ty(n);
            self.register_type(n, ty);
        }
    }

    // 收集 union 声明
    fn collect_union(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var ty = make_ty(n);
            self.register_type(n, ty);
        }
    }

    // 收集 interface 声明
    fn collect_interface(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var ty = make_ty(n);
            self.register_type(n, ty);
        }
    }

    // 收集 fn 声明
    fn collect_fn(self: *mut Self, decl: AstNode) void {
        var name = get_prop(decl.props, "name");
        if (name) |n| {
            var sig = FnSig{
                param_types = Vec<SType>.init(alloc),
                ret_type = make_ty("unknown"),
            };
            self.register_func(n, sig);
        }
    }

    // ========== 检查阶段（第二遍） ==========

    // 检查声明
    fn check_decl(self: *mut Self, decl: AstNode) void {
        var k = decl.kind;
        if (k == "Fn") { self.check_fn(decl); }
        else if (k == "Class") { self.check_class(decl); }
        else if (k == "Namespace") {
            var mut i: usize = 0;
            while (i < decl.children.len) {
                self.check_decl(decl.children[i]);
                i += 1;
            }
        }
    }

    // 检查类声明：逐个检查方法体（self 由 current_class 在 check_fn 内注册）
    fn check_class(self: *mut Self, decl: AstNode) void {
        var cname = get_prop(decl.props, "name");
        if (cname) |c| {
            self.current_class = c;
            var mut i: usize = 0;
            while (i < decl.children.len) {
                var child = decl.children[i];
                if (child.kind == "Fn") { self.check_fn(child); }
                i += 1;
            }
            self.current_class = "";
        }
    }

    // 检查函数声明
    fn check_fn(self: *mut Self, decl: AstNode) void {
        self.push_scope();
        // 解析返回类型是否是错误联合
        var ru = get_prop(decl.props, "ret_union");
        if (ru) |_| { self.current_fn_ret_is_error_union = true; }
        else { self.current_fn_ret_is_error_union = false; }
        // 方法体：注册 self（显式 self 参数会在下方参数循环中覆盖）
        if (self.current_class.len > 0) {
            var self_info = VarInfo{
                ty = make_ty(self.current_class),
                mut_ = true,
            };
            self.register("self", self_info);
        }
        var mut i: usize = 0;
        while (i < decl.children.len) {
            var child = decl.children[i];
            if (child.kind == "Param") {
                var pname = get_prop(child.props, "name");
                if (pname) |n| {
                    var param_ty = self.resolve_ty(child.props);
                    var info = VarInfo{
                        ty = param_ty,
                        mut_ = false,
                    };
                    self.register(n, info);
                }
            }
            i += 1;
        }
        i = 0;
        while (i < decl.children.len) {
            var child = decl.children[i];
            if (child.kind == "Block") {
                self.check_block(child);
            }
            i += 1;
        }
        self.pop_scope();
        self.current_fn_ret_is_error_union = false;
    }

    // 检查块
    fn check_block(self: *mut Self, block: AstNode) void {
        self.push_scope();
        var mut i: usize = 0;
        while (i < block.children.len) {
            self.check_stmt(block.children[i]);
            i += 1;
        }
        self.pop_scope();
    }

    // 检查语句
    fn check_stmt(self: *mut Self, stmt: AstNode) void {
        var k = stmt.kind;
        if (k == "Block") {
            self.check_block(stmt);
        } else if (k == "VarDecl") {
            self.check_var_decl(stmt);
        } else if (k == "If") {
            self.check_if(stmt);
        } else if (k == "While") {
            self.check_while(stmt);
        } else if (k == "For") {
            self.check_for(stmt);
        } else if (k == "Switch") {
            self.check_switch(stmt);
        } else if (k == "Return") {
            self.check_return(stmt);
        } else if (k == "ExprStmt") {
            if (stmt.children.len > 0) {
                self.check_expr(stmt.children[0]);
            }
        } else if (k == "Defer" or k == "Errdefer") {
        } else if (k == "Empty" or k == "Break" or k == "Continue") {
        } else if (k == "ConstDecl") {
            var name = get_prop(stmt.props, "name");
            if (name) |n| {
                var info = VarInfo{
                    ty = make_ty("unknown"),
                    mut_ = false,
                };
                self.register(n, info);
            }
        }
    }

    // 检查变量声明
    fn check_var_decl(self: *mut Self, stmt: AstNode) void {
        var name = get_prop(stmt.props, "name");
        // 解析类型注解：第一个子节点的 kind 是类型名（如 "i32"）
        var mut ty = make_ty("unknown");
        if (stmt.children.len > 0) {
            var first = stmt.children[0];
            var candidate = self.ty_of(first.kind);
            var ck = candidate.kind.as_slice();
            if (ck != "unknown" and ck != "generic") {
                ty = candidate;
            }
        }
        // 判断是否有初始值表达式
        var mut has_init = false;
        if (stmt.children.len > 1) {
            has_init = true;
        } else if (stmt.children.len == 1) {
            var first = stmt.children[0];
            var candidate = self.ty_of(first.kind);
            var ck = candidate.kind.as_slice();
            if (ck == "unknown" or ck == "generic") {
                has_init = true;
            }
        }
        // 检查 mut
        var mut is_mut = false;
        var m = get_prop(stmt.props, "mut");
        if (m) |_| { is_mut = true; }
        if (name) |n| {
            var info = VarInfo{
                ty = ty,
                mut_ = is_mut,
            };
            self.register(n, info);
        }
        // 检查初始值表达式类型（初始值是最后一个子节点）
        if (has_init and stmt.children.len > 0) {
            var last_idx = stmt.children.len - 1;
            var init_expr = stmt.children[last_idx];
            var init_type = self.type_of_expr(init_expr);
            var tk = ty.kind.as_slice();
            var ik = init_type.kind.as_slice();
            if (tk != "unknown" and ik != "unknown") {
                if (!self.is_compatible(init_type, ty)) {
                    var msg = Vec<u8>.init(alloc);
                    msg.append('t'); msg.append('y'); msg.append('p'); msg.append('e');
                    msg.append(' '); msg.append('m'); msg.append('i'); msg.append('s');
                    msg.append('m'); msg.append('a'); msg.append('t'); msg.append('c');
                    msg.append('h'); msg.append(':'); msg.append(' ');
                    msg.append('e'); msg.append('x'); msg.append('p'); msg.append('e');
                    msg.append('c'); msg.append('t'); msg.append('e'); msg.append('d');
                    msg.append(' ');
                    var mut ki: usize = 0;
                    while (ki < ty.kind.len) { msg.append(ty.kind[ki]); ki += 1; }
                    msg.append(','); msg.append(' ');
                    msg.append('g'); msg.append('o'); msg.append('t'); msg.append(' ');
                    ki = 0;
                    while (ki < init_type.kind.len) { msg.append(init_type.kind[ki]); ki += 1; }
                    self.error(msg);
                }
            }
        }
    }

    // 检查条件表达式类型（与 Rust 参考保持一致：接受大多数类型）
    fn check_condition(self: *mut Self, cond: AstNode) void {
        // 当前阶段：条件表达式已在 check_expr 中检查，
        // 此处保留扩展点（未来可添加更严格的类型检查）
    }

    // 检查 if 语句
    fn check_if(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            self.check_expr(stmt.children[0]);
            self.check_condition(stmt.children[0]);
        }
        if (stmt.children.len > 1) {
            var then_block = stmt.children[1];
            if (then_block.kind == "Block") {
                var p = get_prop(stmt.props, "payload");
                if (p) |pn| {
                    self.push_scope();
                    var info = VarInfo{
                        ty = make_ty("unknown"),
                        mut_ = false,
                    };
                    self.register(pn, info);
                    self.check_block(then_block);
                    self.pop_scope();
                } else {
                    self.check_block(then_block);
                }
            }
        }
        if (stmt.children.len > 2) {
            var else_block = stmt.children[2];
            if (else_block.kind == "Block") {
                self.check_block(else_block);
            } else if (else_block.kind == "If") {
                self.check_if(else_block);
            }
        }
    }

    // 检查 while 语句
    fn check_while(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            self.check_expr(stmt.children[0]);
            self.check_condition(stmt.children[0]);
        }
        if (stmt.children.len > 1) {
            var body = stmt.children[1];
            if (body.kind == "Block") {
                var p = get_prop(stmt.props, "payload");
                if (p) |pn| {
                    self.push_scope();
                    var info = VarInfo{
                        ty = make_ty("unknown"),
                        mut_ = false,
                    };
                    self.register(pn, info);
                    self.check_block(body);
                    self.pop_scope();
                } else {
                    self.check_block(body);
                }
            }
        }
    }

    // 检查 for 语句
    fn check_for(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            self.check_expr(stmt.children[0]);
            self.check_condition(stmt.children[0]);
        }
        if (stmt.children.len > 1) {
            var body = stmt.children[1];
            if (body.kind == "Block") {
                // 迭代载荷 `for (xs) \|x\| {...}`：载荷绑定仅限循环体作用域
                var p = get_prop(stmt.props, "payload");
                if (p) |pn| {
                    self.push_scope();
                    var info = VarInfo{
                        ty = make_ty("unknown"),
                        mut_ = false,
                    };
                    self.register(pn, info);
                    self.check_block(body);
                    self.pop_scope();
                } else {
                    self.check_block(body);
                }
            }
        }
    }

    // 检查 switch 语句
    fn check_switch(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            self.check_expr(stmt.children[0]);
        }
        var mut i: usize = 1;
        while (i < stmt.children.len) {
            var arm = stmt.children[i];
            if (arm.kind == "SwitchArm") {
                var mut j: usize = 0;
                while (j < arm.children.len) {
                    self.check_expr(arm.children[j]);
                    j += 1;
                }
            }
            i += 1;
        }
    }

    // 检查 return 语句
    fn check_return(self: *mut Self, stmt: AstNode) void {
        if (stmt.children.len > 0) {
            var expr = stmt.children[0];
            self.check_expr(expr);
            // 检查是否返回局部变量引用（引用逃逸检测）
            if (expr.kind == "AddrOf" and expr.children.len > 0) {
                var inner = expr.children[0];
                if (inner.kind == "Ident") {
                    var name = get_prop(inner.props, "name");
                    if (name) |n| {
                        // 若标识符为作用域内局部变量/参数 → 引用逃逸
                        var found = self.lookup(n);
                        if (found) |_| {
                            var msg = Vec<u8>.init(alloc);
                            append_bytes(&msg, "error: cannot return reference to `");
                            var mut j: usize = 0;
                            while (j < n.len) { msg.append(n[j]); j += 1; }
                            append_bytes(&msg, "`: reference escapes function scope");
                            self.error(msg);
                        }
                    }
                }
            }
            // 检查是否返回错误字面量但函数没有声明错误联合返回类型
            if (expr.kind == "Field" and expr.children.len > 0) {
                var base = expr.children[0];
                if (base.kind == "Ident") {
                    var name = get_prop(base.props, "name");
                    if (name) |n| {
                        if (slice_eq(n, "error")) {
                            if (!self.current_fn_ret_is_error_union) {
                                var msg = Vec<u8>.init(alloc);
                                append_bytes(&msg, "error: cannot return error literal: function does not declare error union");
                                self.error(msg);
                            }
                        }
                    }
                }
            }
        }
    }

    // 检查表达式
    fn check_expr(self: *mut Self, expr: AstNode) void {
        var k = expr.kind;
        if (k == "Ident") {
            self.check_ident(expr);
        } else if (k == "ClassLit") {
            // 只检查各字段初始化值；字段名不作标识符查析（宽容，避免与参考实现诊断分歧）
            var mut i: usize = 0;
            while (i < expr.children.len) {
                var fi = expr.children[i];
                var mut j: usize = 0;
                while (j < fi.children.len) {
                    self.check_expr(fi.children[j]);
                    j += 1;
                }
                i += 1;
            }
        } else if (k == "Binary") {
            if (expr.children.len >= 2) {
                self.check_expr(expr.children[0]);
                self.check_expr(expr.children[1]);
            }
        } else if (k == "Unary") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "Call") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "Field") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "Index") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "Assign") {
        } else if (k == "AddrOf" or k == "Try" or k == "Await") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "ArrayLit") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "IfExpr") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "SwitchExpr") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        } else if (k == "Closure") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                if (expr.children[i].kind == "Block") {
                    self.check_block(expr.children[i]);
                } else {
                    self.check_expr(expr.children[i]);
                }
                i += 1;
            }
        } else if (k == "DotCall") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "Unwrap") {
            if (expr.children.len > 0) {
                self.check_expr(expr.children[0]);
            }
        } else if (k == "AtBuiltin") {
            var mut i: usize = 0;
            while (i < expr.children.len) {
                self.check_expr(expr.children[i]);
                i += 1;
            }
        }
    }

    // 检查标识符引用
    fn check_ident(self: *mut Self, expr: AstNode) void {
        var name = get_prop(expr.props, "name");
        if (name) |n| {
            if (self.is_builtin_name(n)) { return; }
            if (self.types.contains(n)) { return; }
            if (self.funcs.contains(n)) { return; }
            var found = self.lookup(n);
            if (found) |_| {
                return;
            }
            var msg = Vec<u8>.init(alloc);
            msg.append('e'); msg.append('r'); msg.append('r'); msg.append('o'); msg.append('r');
            msg.append(':'); msg.append(' ');
            msg.append('u'); msg.append('n'); msg.append('d'); msg.append('e'); msg.append('f');
            msg.append('i'); msg.append('n'); msg.append('e'); msg.append('d');
            msg.append(' '); msg.append('n'); msg.append('a'); msg.append('m'); msg.append('e');
            msg.append(' '); msg.append('`');
            var mut j: usize = 0;
            while (j < n.len) {
                msg.append(n[j]);
                j += 1;
            }
            msg.append('`');
            self.error(msg);
        }
    }

    // 判断是否为内建名称（使用 slice_eq 避免 &[u8] 指针比较问题）
    fn is_builtin_name(self: *mut Self, name: &[u8]) bool {
        if (slice_eq(name, "alloc") or slice_eq(name, "page_allocator")) return true;
        if (slice_eq(name, "io") or slice_eq(name, "stdout") or slice_eq(name, "stderr")) return true;
        // @ 内建（stage2 lexer 的 AtBuiltin token 文本不含 @，如 intCast）
        if (name.len > 0 and name[0] == '@') return true;
        if (slice_eq(name, "intCast") or slice_eq(name, "floatCast")) return true;
        if (slice_eq(name, "sizeOf") or slice_eq(name, "alignOf") or slice_eq(name, "offsetOf")) return true;
        // 自由内建（对齐 tag1 ops.rs is_free_builtin）
        if (slice_eq(name, "box") or slice_eq(name, "unbox") or slice_eq(name, "copy")) return true;
        if (slice_eq(name, "sqrt") or slice_eq(name, "min") or slice_eq(name, "max")) return true;
        if (slice_eq(name, "fmt_int") or slice_eq(name, "fmt_float") or slice_eq(name, "read_u64_le")) return true;
        if (slice_eq(name, "sort") or slice_eq(name, "binary_search")) return true;
        if (slice_eq(name, "skip_space") or slice_eq(name, "peek") or slice_eq(name, "advance")) return true;
        if (slice_eq(name, "is_digit") or slice_eq(name, "parse_number")) return true;
        if (slice_eq(name, "parse_int") or slice_eq(name, "parse_float") or slice_eq(name, "spawn")) return true;
        if (slice_eq(name, "true") or slice_eq(name, "false") or slice_eq(name, "null") or slice_eq(name, "void")) return true;
        if (slice_eq(name, "pi")) return true;
        if (slice_eq(name, "Vec") or slice_eq(name, "Deque") or slice_eq(name, "Map") or slice_eq(name, "Table")) return true;
        if (slice_eq(name, "String") or slice_eq(name, "Allocator") or slice_eq(name, "ExitType")) return true;
        if (slice_eq(name, "Pipe") or slice_eq(name, "Tee") or slice_eq(name, "Funnel") or slice_eq(name, "Hub")) return true;
        if (slice_eq(name, "i8") or slice_eq(name, "i16") or slice_eq(name, "i32") or slice_eq(name, "i64") or slice_eq(name, "i128")) return true;
        if (slice_eq(name, "u8") or slice_eq(name, "u16") or slice_eq(name, "u32") or slice_eq(name, "u64") or slice_eq(name, "u128")) return true;
        if (slice_eq(name, "isize") or slice_eq(name, "usize")) return true;
        if (slice_eq(name, "f16") or slice_eq(name, "f32") or slice_eq(name, "f64") or slice_eq(name, "f128")) return true;
        if (slice_eq(name, "bool") or slice_eq(name, "void")) return true;
        if (slice_eq(name, "comptime_int") or slice_eq(name, "comptime_float")) return true;
        if (slice_eq(name, "type") or slice_eq(name, "anytype")) return true;
        if (slice_eq(name, "Future")) return true;
        if (slice_eq(name, "expect") or slice_eq(name, "expect_eq")) return true;
        if (slice_eq(name, "error")) return true;
        return false;
    }

    // 输出诊断结果
    fn report(self: *mut Self) void {
        if (self.diags.len == 0) {
            io.print("OK\n");
        } else {
            var mut i: usize = 0;
            while (i < self.diags.len) {
                io.print("{}\n", self.diags[i].as_slice());
                i += 1;
            }
        }
    }
}

// ============================================================
// 入口
// ============================================================

// ============================================================
// AST dump 调试工具（--dump-ast）
// ============================================================
