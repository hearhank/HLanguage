class Counter {
    n: i32,
    fn get(self: *mut Self) i32 {
        return self.n;
    }
}
fn main() {
    var c: Counter = alloc.init(Counter{n = 1});
    io.println(c.get());
}
