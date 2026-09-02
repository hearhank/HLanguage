//! hc-rt/tests/text_rng.rs

use hc_rt::Interp;

fn run_ok(src: &str) {
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed tests: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
}

#[test]
fn text_matches_basic() {
    // 字面 / `.` / 字符类（范围、取反）/ 类速记
    run_ok(
        r##"[test] fn t() !void {
    try expect_eq(io.text.matches("hello", "hello world"), true);
    try expect_eq(io.text.matches("xyz", "hello world"), false);
    try expect_eq(io.text.matches("h.llo", "hello"), true);       // `.` 任意字节
    try expect_eq(io.text.matches("[a-z]+", "hello"), true);
    try expect_eq(io.text.matches("[0-9]+", "hello"), false);
    try expect_eq(io.text.matches("[^a-z]+", "hello"), false);    // 取反类
    try expect_eq(io.text.matches("\\d+", "abc123"), true);        // 数字类
    try expect_eq(io.text.matches("\\d+", "abc"), false);
    try expect_eq(io.text.matches("\\w+", "abc_123"), true);       // 词字符
    try expect_eq(io.text.matches("\\s+", "a b"), true);           // 空白类
    try expect_eq(io.text.matches("^\\.$", "."), true);          // 转义元字符
    try expect_eq(io.text.matches("^\\.$", "x"), false);
}
"##,
    );
}

#[test]
fn text_matches_anchors_alt_quant() {
    // 锚定 / 交替 / 量词（* + ? {n,m}）/ 分组
    run_ok(
        r##"[test] fn t() !void {
    try expect_eq(io.text.matches("^hello", "hello world"), true);
    try expect_eq(io.text.matches("^world", "hello world"), false);
    try expect_eq(io.text.matches("world$", "hello world"), true);
    try expect_eq(io.text.matches("^hello world$", "hello world"), true);
    try expect_eq(io.text.matches("cat|dog", "I have a dog"), true);
    try expect_eq(io.text.matches("cat|dog", "I have a bird"), false);
    try expect_eq(io.text.matches("a+", "caaaat"), true);
    try expect_eq(io.text.matches("colou?r", "color"), true);
    try expect_eq(io.text.matches("colou?r", "colour"), true);
    try expect_eq(io.text.matches("^(ab)+$", "ababab"), true);
    try expect_eq(io.text.matches("^(ab)+$", "aabb"), false);
    try expect_eq(io.text.matches("a{2,3}", "caat"), true);
    try expect_eq(io.text.matches("a{2,3}", "cat"), false);
    try expect_eq(io.text.matches("a{2}", "caat"), true);
    try expect_eq(io.text.matches("a{2,}", "caaaa"), true);
    try expect_eq(io.text.matches("\\d{4}", "year 2026"), true);
    try expect_eq(io.text.matches("^\\d+$", "12345"), true);
    try expect_eq(io.text.matches("^\\d+$", "12a45"), false);
}
"##,
    );
}

#[test]
fn text_find_position() {
    // find：首个匹配起点；无 → null（orelse 给默认）
    run_ok(
        r##"[test] fn t() !void {
    try expect_eq(io.text.find("world", "hello world") orelse -1, 6);
    try expect_eq(io.text.find("xyz", "hello world") orelse -1, -1);
    try expect_eq(io.text.find("\\d+", "abc 123 def") orelse -1, 4);
    try expect_eq(io.text.find("^abc", "abc abc") orelse -1, 0);
    try expect_eq(io.text.find("a+", "zzz") orelse -1, -1);
}
"##,
    );
}

#[test]
fn text_replace_all() {
    // replace：替换全部非重叠匹配（每处最长）；无匹配原样返回
    run_ok(
        r##"[test] fn t() !void {
    try expect_eq_slices(io.text.replace("\\s+", "a   b   c", "-"), "a-b-c");
    try expect_eq_slices(io.text.replace("a+", "aaa", "X"), "X");
    try expect_eq_slices(io.text.replace("a+", "aaabaa", "X"), "XbX");
    try expect_eq_slices(io.text.replace("\\d", "h2o", "#"), "h#o");
    try expect_eq_slices(io.text.replace("\\d+", "item-42-x7", "[n]"), "item-[n]-x[n]");
    try expect_eq_slices(io.text.replace("xyz", "hello", "-"), "hello");
    try expect_eq_slices(io.text.replace("e", "hello", "a"), "hallo");
}
"##,
    );
}

#[test]
fn text_split() {
    // split：按匹配分割（连续分隔符 → 空段；尾部匹配 → 尾空段）
    run_ok(
        r##"[test] fn t() !void {
    var parts = io.text.split(",", "a,b,c");
    try expect_eq(parts.len(), 3);
    try expect_eq_slices(parts[0], "a");
    try expect_eq_slices(parts[1], "b");
    try expect_eq_slices(parts[2], "c");
    var parts2 = io.text.split(",", "a,,b");
    try expect_eq(parts2.len(), 3);
    try expect_eq_slices(parts2[1], "");
    var parts3 = io.text.split(",", "a,");
    try expect_eq(parts3.len(), 2);
    try expect_eq_slices(parts3[1], "");
    var parts4 = io.text.split("\\s+", "one two  three");
    try expect_eq(parts4.len(), 3);
    try expect_eq_slices(parts4[2], "three");
    var parts5 = io.text.split("-", "abc");
    try expect_eq(parts5.len(), 1);
    try expect_eq_slices(parts5[0], "abc");
}
"##,
    );
}

#[test]
fn text_invalid_pattern() {
    // 非法模式（未闭合括号/类、降序量词）→ error.InvalidFormat
    run_ok(
        r##"[test] fn t() !void {
    try expect_error(error.InvalidFormat, io.text.matches("(", "x"));
    try expect_error(error.InvalidFormat, io.text.find("[a-z", "x"));
    try expect_error(error.InvalidFormat, io.text.replace("a{3,2}", "aaa", "X"));
    try expect_error(error.InvalidFormat, io.text.split("*a", "aaa"));
}
"##,
    );
}

#[test]
fn time_tick_elapsed() {
    // tick 大正数（纳秒计数）；elapsed 自 tick 起毫秒数 ≥ 0
    run_ok(
        r##"[test] fn t() !void {
    try expect(io.time.now() > 0);
    var t0 = io.time.tick();
    try expect(t0 > 0);
    try io.time.sleep(2);
    var dt = io.time.elapsed(t0);
    try expect(dt >= 0);
    try expect(dt < 100000);
}
"##,
    );
}

#[test]
fn rng_seed_determinism() {
    // 同种子 → 同序列（xorshift64* 确定流）；seed 可重置
    run_ok(
        r##"[test] fn t() !void {
    io.rng.seed(1);
    var a1 = io.rng.next();
    var a2 = io.rng.next();
    try expect_eq(a1, 0xbafacf624f01c45d);
    try expect_eq(a2, 0x2da6891e507685d);
    io.rng.seed(1);
    try expect_eq(io.rng.next(), a1);
    try expect_eq(io.rng.next(), a2);
    io.rng.seed(0x1234_5678_9abc_def0);
    try expect_eq(io.rng.next(), 0x37fecf87326290b9);
    try expect(a1 != io.rng.next());
}
"##,
    );
}

#[test]
fn rng_int_bounds() {
    // int(n)：[0, n) 均匀；n ≤ 0 → 0
    run_ok(
        r##"[test] fn t() !void {
    io.rng.seed(42);
    var mut i = 0;
    while (i < 100) {
        var v = io.rng.int(10);
        try expect(v >= 0);
        try expect(v < 10);
        i += 1;
    }
    try expect_eq(io.rng.int(1), 0);
    try expect_eq(io.rng.int(0), 0);
    try expect_eq(io.rng.int(-5), 0);
}
"##,
    );
}

#[test]
fn rng_float_range() {
    // float()：[0, 1)
    run_ok(
        r##"[test] fn t() !void {
    io.rng.seed(7);
    var mut i = 0;
    while (i < 100) {
        var f = io.rng.float();
        try expect(f >= 0.0 && f < 1.0);
        i += 1;
    }
}
"##,
    );
}

#[test]
fn timezone_components_format() {
    // 时区完整（A4）：UTC 日历分量 + ISO 8601 格式化
    run_ok(
        r##"[test] fn t() !void {
    var ts = io.time.now();
    try expect(ts > 0);

    // 测试日历分量
    var comp = io.time.components(ts);
    try expect(comp.year >= 2026);
    try expect(comp.year < 2100);
    try expect(comp.month >= 1 && comp.month <= 12);
    try expect(comp.day >= 1 && comp.day <= 31);
    try expect(comp.hour >= 0 && comp.hour <= 23);
    try expect(comp.min >= 0 && comp.min <= 59);
    try expect(comp.sec >= 0 && comp.sec <= 59);
    try expect(comp.ms >= 0 && comp.ms <= 999);

    // 测试格式化
    var fmt = io.time.format(ts);
    try expect(fmt.len > 10);
    // 格式应为 ISO 8601: YYYY-MM-DDTHH:MM:SS.mmmZ
    try expect(fmt[4] == 45); // '-'
    try expect(fmt[7] == 45); // '-'
    try expect(fmt[10] == 84); // 'T'
}
"##,
    );
}
