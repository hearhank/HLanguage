#!/bin/bash
# ============================================================
# K5 S8: stage2 self-bootstrap closed loop - host chain (fast)
#   (ADR-0033: oracle/resume explanation-chain modes retired;
#    the binary chain supersedes them - see
#    docs/SPEC/phase4/09-bootstrap-binary-chain-plan.md)
#
#   fast [default] host chain: Rust hc runs the stage2
#   compiler via package mode - tree-walking, ~21s.
#   Produces A.hbc, then Phase B + V1. Daily gate.
#
#   Phase B: A.hbc on the HBC2 VM recompiles the same sources
#            -> B.hbc. Assert V1: A == B byte-identical (cmp).
# Progress: stage2/test/progress.txt (markers per file/phase)
# Usage: ./bootstrap.sh
# Run from the repo root (paths are repo-relative).
# ============================================================

set -e

SRC="stage2/src/main.hc stage2/src/ir.hc stage2/src/lower.hc stage2/src/encode.hc stage2/src/lexer.hc stage2/src/parser.hc stage2/src/checker.hc"
A="stage2/test/A.hbc"
B="stage2/test/B.hbc"

run_cmd() {
    echo "+ $*"
    time "$@"
}

echo "[A] host chain: Rust hc runs the stage2 compiler - tree-walking, ~21s"
run_cmd hc run stage2 --emit-hbc "$A" $SRC

echo "[B] $A on the HBC2 VM recompiles stage2..."
run_cmd hc run "$A" --emit-hbc "$B" $SRC

echo "[V1] cmp $A $B"
if cmp -s "$A" "$B"; then
    echo "V1 PASS: byte-identical"
    exit 0
else
    echo "V1 FAIL: A != B"
    exit 1
fi
