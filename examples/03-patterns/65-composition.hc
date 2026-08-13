// 65-composition.hc — 组合（12.20：组合优于继承，无继承）
//
//   - class 组合：字段持有其它复杂类型（默认拥有，Q16）
//   - 接口组合：一个类型实现多个接口（Q14 冒号标注）

interface Drawable {
    fn draw(self: *Self) void;
}

interface Saveable {
    fn save(self: *Self, io: Io) !void;
}

class Paragraph {
    mut text: String,
    mut style: TextStyle,        // 组合：段落 = 文本 + 样式

    fn draw(self: *Self) void {
        // 绘制逻辑
    }
}

class Document: Drawable, Saveable {
    mut title: String,
    mut body: Vec(Paragraph),    // 组合：文档 = 标题 + 段落列表

    fn draw(self: *Self) void {
        for (self.body) |p| {
            p.draw();
        }
    }

    fn save(self: *Self, io: Io) !void {
        // 保存逻辑（序列化 + 落盘）
    }
}

fn main(io: Io) !void {
    var doc: o Document = Document.new(alloc);
    doc.title = String.from("组合示例", alloc);
    doc.body.append(Paragraph.new(alloc));
    io.print("paragraphs = {}\n", doc.body.len);
}
