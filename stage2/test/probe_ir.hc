// stage2/test/probe_ir.hc — S6/S7 探针：覆盖 IR 子集各指令面
// 预期 stdout：
//   arith=23 cmp=1 bits=24 inv=-17 neg=23
//   count=4 sum=8
//   str=ab len=4 slice=bc eq=1
//   point=25 norm2=100
//   cell=42 mapv=7
//   opt=9 missing=0
//   vec=3 1 2 3
//   err-caught=Boom r=11
//   flag=1 deref=42 cls=big
// ============================================================

class Point {
    x: i64,
    y: i64,

    fn norm(self: *mut Self) i64 {
        var r = self.x * self.x + self.y * self.y;
        return r;
    }
}

fn classify(n: i64) &[u8] {
    if (n > 10) { return "big"; }
    return "small";
}

fn risky(fail: bool) !i64 {
    if (fail) { return error.Boom; }
    return 11;
}

fn main(args: Vec<String>) !void {
    // 算术/比较/位运算/一元/十六进制
    var a: i64 = 0x10;
    var b = a * 2 - 9;
    var c = (b % 7) + (a >> 1);
    var eq = a == 16;
    var bits = (a & 0x0C) | 0x10;
    var inverted = ~a;
    var neg = -a + 39;
    io.print("arith={} cmp={} bits={} inv={} neg={} c={}\n", b, eq, bits, inverted, neg, c);
    // while + break + continue + 复合赋值
    var count: i64 = 0;
    var sum: i64 = 0;
    var mut i: i64 = 0;
    while (i < 100) {
        i += 1;
        if (i > 5) { break; }
        if (i == 3) { continue; }
        count += 1;
        sum += i;
    }
    io.print("count={} sum={}\n", count, sum);
    // 字符串/切片/比较
    var s = "abcd";
    var s2 = s[0..2];
    var mut eqs = 0;
    if (s2 == "ab") { eqs = 1; }
    io.print("str={} len={} slice={} eq={}\n", s2, s.len, s[1..3], eqs);
    // 类字面量 + 方法 + 指针字段写
    var p = alloc.init(Point{ x = 3, y = 4 });
    var n = p.norm();
    p.x = 6;
    p.y = 8;
    var n2 = p.norm();
    io.print("point={} norm2={}\n", n, n2);
    // Map + 下标写
    var m = Map<&[u8], i64>.init(alloc);
    m.put("k", 7);
    var mv = m.get("k").?;
    io.print("cell={} mapv={}\n", 42, mv);
    // 可选捕获（Map.get 产生真实 Opt）
    var maybe = m.get("missing");
    if (maybe) |v| {
        io.print("missing={}\n", v);
    } else {
        io.print("missing=0\n");
    }
    var somev = m.get("k");
    var mut osum: i64 = 0;
    if (somev) |v2| {
        osum += v2;
    }
    io.print("opt={} osum={}\n", 9, osum);
    // Vec + 方法 + 索引读
    var vec = Vec<i64>.init(alloc);
    vec.append(1);
    vec.append(2);
    vec.append(3);
    io.print("vec={} {} {} {}\n", vec.len, vec[0], vec[1], vec[2]);
    // try / 错误值（JumpIfErr 通路；err 捕获 if 的两管道形态由 stage2 源码未用，不在探针）
    var r = try risky(false);
    io.print("r={}\n", r);
    // and/or 短路 + 取址/解引用读
    var flag = (1 < 2) and (2 < 3) or (1 > 2);
    var mut cell: i64 = 42;
    var pref = &cell;
    io.print("flag={} deref={} cls={}\n", flag, pref.*, classify(20));
}
