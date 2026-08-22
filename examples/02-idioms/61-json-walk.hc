import H.std.{io};

// 61-json-walk.hc — JSON 值遍历（enum 递归，54 延伸）
//
//   - JsonValue：enum 负载递归（数组/对象元素也是 JsonValue）
//   - switch 穷举 + 递归遍历（Q27 switch 表达式）

enum JsonValue {
    null,
    boolean: bool,
    number: f64,
    string: String,
    array: Vec<JsonValue>,       // 递归
    object: Vec<JsonPair>,       // 简化：键值对列表
}

class JsonPair {
    key: String,
    value: JsonValue,
}

fn count_strings(v: *JsonValue) i32 {
    return switch (v.*) {
        JsonValue.null => 0,
        JsonValue.boolean => |_| 0,
        JsonValue.number => |_| 0,
        JsonValue.string => |_| 1,
        JsonValue.array => |items| {
            var n = 0;
            for (items) |item| {
                n += count_strings(&item);
            }
            return n;
        },
        JsonValue.object => |pairs| {
            var n = 0;
            for (pairs) |p| {
                n += count_strings(&p.value);
            }
            return n;
        },
    };
}

fn main() !void {
    // 构造一个简单 JSON：{"a": "x", "list": [1, "y"]}
    var arr = Vec<JsonValue>.init(alloc);
    arr.append(JsonValue{number = 1.0});
    arr.append(JsonValue{string = String.from("y", alloc)});

    var pairs = Vec<JsonPair>.init(alloc);
    pairs.append(JsonPair{key = String.from("a", alloc), value = JsonValue{string = String.from("x", alloc)}});
    pairs.append(JsonPair{key = String.from("list", alloc), value = JsonValue{array = arr}});

    var doc = JsonValue{object = pairs};
    io.print("strings = {}\n", count_strings(&doc));   // 2
}

[test] fn json_walk_count() !void {
    var arr = Vec<JsonValue>.init(alloc);
    arr.append(JsonValue{number = 1.0});
    arr.append(JsonValue{string = String.from("y", alloc)});

    var pairs = Vec<JsonPair>.init(alloc);
    pairs.append(JsonPair{key = String.from("a", alloc), value = JsonValue{string = String.from("x", alloc)}});
    pairs.append(JsonPair{key = String.from("list", alloc), value = JsonValue{array = arr}});

    var doc = JsonValue{object = pairs};
    try expect_eq(count_strings(&doc), 2);   // "x" + "y"
}
