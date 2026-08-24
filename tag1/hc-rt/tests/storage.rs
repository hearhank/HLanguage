//! hc-rt/tests/storage.rs

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
fn storage_kv_put_get() {
    // 键值存储基本流：put → get（orelse 取回）；get 缺失 → null
    run_ok(
        r#"[test] fn t() !void {
    var kv = try io.storage.open("hc_g4_kv1.dat");
    try kv.put("name", "hank");
    var v = try kv.get("name");
    try expect_eq_slices(v orelse "", "hank");
    var miss = try kv.get("nope");
    try expect_eq_slices(miss orelse "default", "default");
    try kv.close();
    try io.fs.remove("hc_g4_kv1.dat");
}
"#,
    );
}

#[test]
fn storage_kv_contains_remove_len() {
    // contains / remove（幂等）/ len 生命周期
    run_ok(
        r#"[test] fn t() !void {
    var kv = try io.storage.open("hc_g4_kv2.dat");
    try expect_eq(kv.len(), 0);
    try kv.put("a", "1");
    try kv.put("b", "2");
    try kv.put("c", "3");
    try expect_eq(kv.len(), 3);
    try expect_eq(kv.contains("a"), true);
    try expect_eq(kv.contains("z"), false);
    try kv.remove("a");
    try expect_eq(kv.contains("a"), false);
    try expect_eq(kv.len(), 2);
    try kv.remove("a"); // 幂等：再删不存在键为 no-op
    try expect_eq(kv.len(), 2);
    try kv.close();
    try io.fs.remove("hc_g4_kv2.dat");
}
"#,
    );
}

#[test]
fn storage_kv_persist_reopen() {
    // 持久化：close 落盘 → reopen 读回既有条目
    run_ok(
        r#"[test] fn t() !void {
    var kv = try io.storage.open("hc_g4_kv3.dat");
    try kv.put("k1", "v1");
    try kv.put("k2", "v2");
    try kv.close();
    var kv2 = try io.storage.open("hc_g4_kv3.dat");
    try expect_eq(kv2.len(), 2);
    try expect_eq_slices(try kv2.get("k1") orelse "", "v1");
    try expect_eq_slices(try kv2.get("k2") orelse "", "v2");
    try kv2.close();
    try io.fs.remove("hc_g4_kv3.dat");
}
"#,
    );
}

#[test]
fn storage_kv_close_idempotent() {
    // close 幂等：重复 close 为 no-op（不报 BadFd）
    run_ok(
        r#"[test] fn t() !void {
    var kv = try io.storage.open("hc_g4_kv4.dat");
    try kv.put("x", "1");
    try kv.close();
    try kv.close();
    try io.fs.remove("hc_g4_kv4.dat");
}
"#,
    );
}

#[test]
fn archive_compress_decompress_roundtrip() {
    // RLE round-trip：重复段压缩明显变短；解压还原
    run_ok(
        r#"[test] fn t() !void {
    var c = try io.archive.compress("aaabbbccccc");
    try expect(c.len < 11);
    var d = try io.archive.decompress(c);
    try expect_eq_slices(d, "aaabbbccccc");
    var big = try io.archive.compress("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
    try expect(big.len < 30);
    try expect_eq_slices(try io.archive.decompress(big), "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
}
"#,
    );
}

#[test]
fn archive_binary_roundtrip() {
    // 任意二进制 round-trip：token 字节（0x00/0x01）出现在字面数据中仍保真
    run_ok(
        r#"[test] fn t() !void {
    try expect_eq_slices(try io.archive.decompress(try io.archive.compress("\x00\x01\x02\x03\x04\x05")), "\x00\x01\x02\x03\x04\x05");
    try expect_eq_slices(try io.archive.decompress(try io.archive.compress("\x00\x00\x00\x00")), "\x00\x00\x00\x00");
    try expect_eq_slices(try io.archive.decompress(try io.archive.compress("\x01\x01\x01\x01\x01")), "\x01\x01\x01\x01\x01");
}
"#,
    );
}

#[test]
fn archive_decompress_invalid() {
    // 非法压缩数据 → error.InvalidFormat
    run_ok(
        r#"[test] fn t() !void {
    try expect_error(error.InvalidFormat, io.archive.decompress("\x00"));
    try expect_error(error.InvalidFormat, io.archive.decompress("\x01\x00"));
    try expect_error(error.InvalidFormat, io.archive.decompress("\x00\xff\x00"));
}
"#,
    );
}
