class L {
    mut pos: i32,
}
fn main() {
    var l: L = alloc.init(L{pos = 0});
    l.run();
}
