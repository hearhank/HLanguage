import H.std.{io};

// K2a 类型系统：Owned / Ptr / Slice / Optional / ErrorUnion / Tuple / Array
// / 泛型 / ComptimeInt / FnN / 限定名 / anytype / type / void

global a: i32;
global b: *mut u8;
global c: &[u8];
global d: &mut [i32];
global e: ?i32;
global f: !void;
global g: o Vec(String);
global h: [4]i32;
global i: (i32, f64);
global j: Vec(i32);
global k: Vec(Vec(u8));
global l: H.std.io.Reader;
global m: Fn2(i32, i32) i32;
global n: Fn0() void;
global selfptr: *Self;
global p: ??i32;
global q: !?i32;
global r: *!i32;
global s: [8]u8;
global t: (i32, (f64, f64));

interface IEvery {
    fn take(
        self: *mut Self,
        a: o Vec(String),
        b: [3]i32,
        c: ?i32,
        d: !void,
        e: (f64, f64),
        f: Fn1(i32) i32,
        g: Vec(Vec(i32)),
    ) void;
}

enum Tagged {
    None,
    One: i32,
    Two: (i32, i32),
    Three: Vec(u8),
}

union Bits {
    n: i32,
    p: *mut u8,
    arr: [8]u8,
}
