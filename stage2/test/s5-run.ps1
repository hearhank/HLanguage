# stage2/test/s5-run.ps1 — S5 挂机运行（ADR-0033 产物链）
# interp.hbc（IR VM -> interp）解释执行 stage2 编译器，编译 stage2 自身 -> A2.hbc
# 每行实时加时间戳（绝对时刻 + 相对秒），同步落盘 stage2/test/s5.log
# 完成后自动断言 S5: A2 == A.hbc（字节级）。
#
# 用法: powershell -ExecutionPolicy Bypass -File stage2\test\s5-run.ps1
# （任意工作目录均可——脚本自动定位仓库根）

# 自动定位仓库根（本脚本位于 <root>/stage2/test/ 下）
Set-Location (Join-Path $PSScriptRoot "..")
Set-Location (Join-Path (Get-Location) "..")
Write-Output ("repo root: " + (Get-Location).Path)

$ErrorActionPreference = "Continue"
$t0 = Get-Date

$src = @(
    "stage2/src/main.hc",
    "stage2/src/ir.hc",
    "stage2/src/lower.hc",
    "stage2/src/encode.hc",
    "stage2/src/lexer.hc",
    "stage2/src/parser.hc",
    "stage2/src/checker.hc"
)

Write-Output ("S5 start: " + $t0.ToString("yyyy-MM-dd HH:mm:ss"))

& hc run stage2/test/interp.hbc stage2/src/main.hc --emit-hbc stage2/test/A2.hbc $src 2>&1 |
    ForEach-Object {
        '{0:HH:mm:ss} +{1,7:N0}s {2}' -f (Get-Date), ((Get-Date) - $t0).TotalSeconds, $_
    } | Tee-Object -FilePath stage2/test/s5.log

if ($LASTEXITCODE -ne 0) {
    Write-Output ("S5 FAIL: compiler exited with " + $LASTEXITCODE)
    exit 1
}

# S5 断言：A2 == A.hbc（字节级）
$a = [System.IO.File]::ReadAllBytes("stage2/test/A2.hbc")
$b = [System.IO.File]::ReadAllBytes("stage2/test/A.hbc")
if ($a.Length -ne $b.Length) {
    Write-Output ("S5 FAIL: size differs A2=" + $a.Length + " A=" + $b.Length)
    exit 1
}
if ([System.Linq.Enumerable]::SequenceEqual($a, $b)) {
    Write-Output ("S5 PASS: A2 == A byte-identical (" + $a.Length + " bytes), total " + [int]((Get-Date) - $t0).TotalSeconds + "s")
    exit 0
}
$first = 0
while ($a[$first] -eq $b[$first]) { $first++ }
Write-Output ("S5 FAIL: first differing byte at offset " + $first)
exit 1
