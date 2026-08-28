fn classify(b: u8) u8 {
    if (b == 'a') { return 1; } else if (b == 'b') { return 2; } else if (b == 'c') { return 3; } else if (b == 'd') { return 4; } else if (b == 'e') { return 5; } else { return 0; }
}
fn main() {
    io.print("{}\n", classify('c'));
}
