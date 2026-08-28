class L {
    mut pos: i32,
    fn bump(self: *mut Self) void {
        self.pos += 1;
    }
}
fn main() {
    var l: L = alloc.init(L{pos = 0});
}
