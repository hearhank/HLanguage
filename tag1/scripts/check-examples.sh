#!/usr/bin/env bash
# 完整示例套件回归门（CI 与本地共用）。
#
# 两部分（基线随功能扩增而更新，见 tag1/README.md）：
#   1) interpret：`hc test examples/` 断言 >= 125 passed 且 <= 11 failed（125/136）
#   2) compile：`hc test --mode=compile examples/` 断言 <= 54 mismatch
#      （未实现原生内建/方法 → error.NotBuiltin/NoMethod 响亮中止；子集扩增时该数下降，属改进）
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
if [ "$mismatch" -gt 54 ]; then
    echo "::error::编译交叉验证回归：$mismatch mismatch（基线 <=54）"
    exit 1
fi
echo "compile OK: $mismatch mismatch"
