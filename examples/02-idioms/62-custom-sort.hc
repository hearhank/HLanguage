// 62-custom-sort.hc — 自定义比较器排序（闭包作比较器，12.9）
//
//   - sort 接受比较器闭包（返回 i32 序）
//   - 按不同字段/方向排序（Fn2 调用接口，Q13）

struct Person {
    name: String,
    age: i32,
}

fn main(io: Io) !void {
    var people = Vec(Person).init(alloc);
    people.append(Person{ name = String.from("alice", alloc), age = 30 });
    people.append(Person{ name = String.from("bob", alloc), age = 25 });
    people.append(Person{ name = String.from("carol", alloc), age = 35 });

    // 按年龄升序（闭包比较器）
    sort(&mut people, |a, b| a.age - b.age);

    // 按姓名（字符串比较）
    sort(&mut people, |a, b| String.compare(a.name, b.name));

    for (people) |p| {
        io.print("{}: {}\n", p.name, p.age);
    }
}
