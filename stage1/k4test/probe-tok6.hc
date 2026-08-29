// probe-tok6.hc — S2 诊断：Vec<class> 多次 append 触发重分配后，非空 text 字段是否存活
class T6 {
    kind: &[u8],
    text: Vec<u8>,
    start: usize,
}

fn mk(t: &[u8], k: &[u8], s: usize) T6 {
    var txt = Vec<u8>.init(alloc);
    var mut i: usize = 0;
    while (i < t.len) {
        txt.append(t[i]);
        i += 1;
    }
    return T6{ kind = k, text = txt, start = s };
}

fn main(args: Vec<String>) !void {
    var toks = Vec<T6>.init(alloc);
    toks.append(mk("tok0", "KwFn", 0));
    toks.append(mk("tok1", "Ident", 1));
    toks.append(mk("tok2", "Ident", 2));
    toks.append(mk("tok3", "Ident", 3));
    toks.append(mk("tok4", "Ident", 4));
    toks.append(mk("tok5", "Ident", 5));
    toks.append(mk("tok6", "Ident", 6));
    toks.append(mk("tok7", "Ident", 7));
    toks.append(mk("tok8", "Ident", 8));
    toks.append(mk("tok9", "Ident", 9));
    var mut i: usize = 0;
    while (i < toks.len) {
        var t = toks[i];
        var kb = Vec<u8>.init(alloc);
        var mut ki: usize = 0;
        while (ki < t.text.len) { kb.append(t.text[ki]); ki += 1; }
        io.print("i={} text={} start={}\n", @intCast(i64, i), kb.as_slice(), t.start);
        i += 1;
    }
}
