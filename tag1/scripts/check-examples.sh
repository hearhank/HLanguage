#!/usr/bin/env bash
# 完整示例套件回归门（CI 与本地共用）。
#
# 两部分（基线随功能扩增而更新，见 tag1/README.md）：
#   1) interpret：`hc test examples/` 断言 >= 125 passed 且 <= 11 failed（143/148 通过 + 1 跳过：
#      23-tests 的 skip_example 自 F1 起实际触发 error.SkipTest → SKIP，不计入 passed/failed；
#      130→132 为 H3 新增 91-orders-domain（[module] 领域约定，2 测试全绿）；
#      132→133 为组 D（E1.2）comptime 类型函数：34-generics 由失败转全绿；
#      133→135 为组 D（E1.2）D3/D4 最小切片：35-comptime-branch（comptime_int 值参数 +
#      数组类型函数 `[n]T`）由失败转全绿，失败 9→8；
#      135→142 为组 E E1：async/await 解析 + 语义落地——含 `async fn`/`await` 的 5 例
#      （37/38/39/76/80）由双后端解析失败转为解释执行（async 体同步执行 + await 透传，
#      E2 前近似正确），失败 8→5；
#      142→143 为捕获语法解析落地：79-retry 由解析失败转全绿（retry_demo），失败 5→4）
#   2) compile：`hc test --mode=compile examples/` 断言 <= 60 mismatch
#      （未实现原生内建/方法 → error.NotBuiltin/NoMethod 响亮中止；子集扩增时该数下降，属改进。
#      52→53 为 D1 副作用：interpret 侧 fmt_int 修复使 63-template-render 转绿，原生侧
#      String.from/replace/find 仍缺 → 该例由双失败转为 mismatch。
#      53→54 为 G1 副作用：`spawn(f, …)` 解析落地，77-producer-consumer 由双解析失败转为
#      interpret 运行至 error.UndefinedName（四模式类型 OneToOne 未实现，第三块）而原生
#      LLVM 在 spawn 处 error.Unsupported 拒绝 → 计入 mismatch 的 +1；两后端均失败不变。
#      54→55 为 G5 副作用：新增在列示例 90-thread-lifecycle（组 G 线程，interpret 全绿），
#      原生子集边界（spawn 需 FnRef → error.NotCallable，Phase 8 ABI）→ 该例 1 mismatch。
#      原生线程支持留 Phase 8 原生 ABI 改造（G4b 定案 A，见 tag1/README.md）。
#      55→53 为组 D（E1.2）：类型函数体降级跳过（comptime-only）+ NamedLit 具体化名，
#      34-generics 原生编译转绿（降 2 mismatch——interpret 与 compile 双计数消除）；
#      53→58 为组 E E1 副作用：async/await 解析落地使含 `async fn`/`await` 的 5 例
#      （37/38/39/76/80）由双后端解析失败转为 interpret 绿 + 原生红——原生/IR 后端尚无
#      Future/async 与四模式容器（ManyToMany 等），error.Unsupported 响亮中止。
#      组 E E2-E4 后：5 例的 `[test]` 异步断言已在双后端（interpret + compile）全绿——
#      IR 侧 async fn 调用同步执行 + await 透传（子集边界）对齐纯函数结果；剩余 58
#      mismatch 中 5 例的文件级 MISMATCH 来自 `main` 函数特性而非 async：四模式容器
#      （37/76 行 26/29，ManyToMany/OneToOne——组 F 延迟 1.x）、38/80（G1 net 已
#      落地仍红：38 主函数旧 URL 形式 connect(url)/read_all(&conn) + JsonValue 类型
#      未实现，80 主函数 https:// 网络不可达——仅 http:// 支持，均非 G1 范围）、
#      Io.evented 原生构造器（39，interp-only E3）——58 保持，回落依赖 F/G 落地而非 E 组；
#      58→60 为捕获语法解析副作用：78-task-dispatch / 79-retry 由双解析失败转为可解析——
#      04-concurrency 同组包级原生编译因 76 的 four_mode_types（组 F 四模式类型，延迟 1.x）
#      在原生 LLVM 处 error.Unsupported 响亮拒绝 → 组内每个已解析文件均计入文件级 MISMATCH，
#      78/79 各 +1；79 单独原生编译已验证 MATCH（捕获语法本身原生可编），78 的失败模式由
#      解析错误转为运行时 error.UndefinedName（组 F 四模式类型，延迟 1.x））
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
if [ "$mismatch" -gt 60 ]; then
    echo "::error::编译交叉验证回归：$mismatch mismatch（基线 <=60）"
    exit 1
fi
echo "compile OK: $mismatch mismatch"
