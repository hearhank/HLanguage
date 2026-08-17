import H.std.{io};

// 33-script.hc — 脚本生成（数据定义 → 代码，E1 就地替换）
//
// Q17 定案（2026-08-13）：就地替换
//   - script { ... } 块在编译前被生成结果整体替换（H5：编译前执行、模板生成）
//   - 编辑器分屏实时显示「脚本 ↔ 生成物」；编译时生成物参与编译
//   - 脚本 = H 核心子集（无运行时环境，Q13/B1）；类型信息可见 = 所在作用域（H5）
// Q23 定案（2026-08-13）：输入机制 = 隐式 types 元数据对象
//   - types.fields("Person") 返回字段 [名, 类型] 列表；产物 = 代码字符串就地替换

class Person {   // 含 String 字段 → 非 Continuous（默认 class，堆上）
    name: String,
    age: i32,
}

// 数据定义 → 生成「字段清单函数」（就地替换本块；产物 = 代码字符串）
script {
    var count = 0;
    var out = "";
    for (types.fields("Person")) |f| {
        count = count + 1;
        out = out.concat("// field ").concat(f[0]).concat(": ").concat(f[1]).concat("\n");
    }
    out.concat("fn person_field_count() i32 { return ").concat(String.from(count)).concat("; }");
}

fn main(args: o Vec(String)) !void {
    var p = alloc.init(Person{name = String.from("alice", alloc), age = 30});   // 带参构造（C1'）
    io.print("person_field_count = {}\n", person_field_count());
}

[test] fn script_generation_demo() !void {
    // S4 演示型（Q-T6）：person_field_count 由脚本生成（Q23 types 元数据），
    // 由本块的声明文本区间就地替换（Q17）
    try expect(person_field_count() == 2);
}
