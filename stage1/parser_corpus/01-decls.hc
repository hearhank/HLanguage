import H.std.{io as stdio};
import H.std.{io};
import H.std as hstd;

const PI: f64 = 3.14;
const SET = error{E1, E2};
const U = A || B;

global ver: i32 = 1;
global name: &[u8];
global raw = 42;

using Math;
using Math as M;

namespace Math {
    const MAX: i32 = 100;
    global PI: f64 = 3.14;
    namespace Deep {
        const D: i32 = 1;
    }
}

[module] namespace Internal {
    const SECRET: i32 = 42;
}

interface Reader {
    fn read(self: *mut Self, buf: &[u8]) i32;
    fn close(self: *mut Self);
}

interface Named : Base, Clone {
    fn get(self: *mut Self) String;
}

enum Color {
    Red,
    Green,
    Blue,
}

enum Shape {
    Circle: f64,
    Rect: (f64, f64),
}

union Value {
    i: i32,
    f: f64,
    mut s: *mut u8,
}
