// 字节化（to_bytes/from_bytes）：C 后端验证（双后端一致性）
// h run examples/bytes.hc 与 h build examples/bytes.hc --exec 输出必须完全一致
// 字节格式 = 可逆自描述 JSON（与求值器 JSON.stringify 逐字节一致）

struct Item {
    name: Str
    price: f64
}

class Account {
    mut balance: f64
    id: u64
    label: Str
    items: [Item]
}

fun main() -> void {
    mut acc = Account{ balance: 100.0, id: 7, label: "主账户", items: [Item{ name: "咖啡", price: 3.5 }] }
    bytes = acc.to_bytes()
    print("字节:", bytes)
    restored = Account.from_bytes(bytes)
    print("恢复:", restored)
    print("恢复字段:", restored.id.to_str(), restored.label.to_str())
    print("完成")
}
