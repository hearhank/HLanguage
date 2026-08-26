import H.std.{io};

// 62-custom-sort.hc — 自定义比较器排序（闭包作比较器，12.9）
//
//   - sort 接受比较器闭包（返回 i32 序）
//   - 按不同字段/方向排序（Fn2 调用接口，Q13）

class Person {   // 含 &[u8] 字段 → 非 Continuous（默认 class，堆上）
    name: &[u8],
    age: i32,
}

fn main() !void {
    var people = Vec<Person>.init(alloc);
    people.append(alloc.init(Person{name = "alice", age = 30}));
    people.append(alloc.init(Person{name = "bob", age = 25}));
    people.append(alloc.init(Person{name = "carol", age = 35}));

    // 按年龄升序（闭包比较器）
    sort(&mut people, |a, b| a.age - b.age);

    // 按姓名（字符串比较）
    sort(&mut people, |a, b| String.compare(a.name, b.name));

    for (people) |p| {
        io.print("{}: {}\n", p.name, p.age);
    }
}

[Test] fn custom_comparator_sort() !void {
    var people = Vec<Person>.init(alloc);
    people.append(alloc.init(Person{name = "alice", age = 30}));
    people.append(alloc.init(Person{name = "bob", age = 25}));
    people.append(alloc.init(Person{name = "carol", age = 35}));
    sort(&mut people, |a, b| a.age - b.age);   // 按年龄升序
    try expect_eq(people[0].age, 25);   // bob
    try expect_eq(people[2].age, 35);   // carol
}