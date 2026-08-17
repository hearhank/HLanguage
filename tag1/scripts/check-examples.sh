#!/usr/bin/env bash
# 完整示例套件回归门（CI 与本地共用）。
#
# 两部分（基线随功能扩增而更新，见 tag1/README.md）：
#   1) interpret：`hc test examples/` 断言 >= 125 passed 且 <= 11 failed（132/143 通过 + 1 跳过：
#      23-tests 的 skip_example 自 F1 起实际触发 error.SkipTest → SKIP，不计入 passed/failed；
#      130→132 为 H3 新增 91-orders-domain（[module] 领域约定，2 测试全绿））
#   2) compile：`hc test --mode=compile examples/` 断言 <= 55 mismatch
#      （未实现原生内建/方法 → error.NotBuiltin/NoMethod 响亮中止；子集扩增时该数下降，属改进。
#      52→53 为 D1 副作用：interpret 侧 fmt_int 修复使 63-template-render 转绿，原生侧
#      String.from/replace/find 仍缺 → 该例由双失败转为 mismatch。
#      53→54 为 G1 副作用：`spawn(f, …)` 解析落地，77-producer-consumer 由双解析失败转为
#      interpret 运行至 error.UndefinedName（四模式类型 OneToOne 未实现，第三块）而原生
#      LLVM 在 spawn 处 error.Unsupported 拒绝 → 计入 mismatch 的 +1；两后端均失败不变。
#      54→55 为 G5 副作用：新增在列示例 90-thread-lifecycle（组 G 线程，interpret 全绿），
#      原生子集边界（spawn 需 FnRef → error.NotCallable，Phase 8 ABI）→ 该例 1 mismatch。
#      原生线程支持留 Phase 8 原生 ABI 改造（G4b 定案 A，见 tag1/README.md））
#
# 用法：bash tag1/scripts/check-examples.sh（工作目录不限，脚本自定位到 tag1/）
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.." # tag1/

echo "== 完整示例套件（interpret）=="
out="$(cargo run -q -p hc-tools -- test ../examples/ 2>&1 || true)"
echo "$out" | tail -15

passed="$(echo "$out" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' | head -1)"
failed="$(echo "$out" | grep -oE '[0-9]+ failed' | grep -oE '[0-9]+' | head -1)"

if [ -z "$passed" ] || [ -z "$failed" ]; then
    echo "::error::无法解析 interpret 汇总（passed=$passed failed=$failed）"
    exit 1
fi
if [ "$passed" -lt 125 ] || [ "$failed" -gt 11 ]; then
    echo "::error::示例套件回归：$passed passed / $failed failed（基线 >=125 / <=11）"
    exit 1
fi
echo "interpret OK: $passed passed / $failed failed"

echo
echo "== 编译交叉验证（compile，需 zig cc）=="
out2="$(cargo run -q -p hc-tools -- test --mode=compile ../examples/ 2>&1 || true)"
echo "$out2" | tail -15

mismatch="$(echo "$out2" | grep -oE '[0-9]+ mismatch' | grep -oE '[0-9]+' | head -1)"
if [ -z "$mismatch" ]; then
    echo "::error::无法解析 compile mismatch 汇总（可能缺少 zig cc）"
    exit 1
fi
if [ "$mismatch" -gt 55 ]; then
    echo "::error::编译交叉验证回归：$mismatch mismatch（基线 <=55）"
    exit 1
fi
echo "compile OK: $mismatch mismatch"
