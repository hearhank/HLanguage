#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Migrate H generic type syntax from `Name(...)` to `Name<...>`.

Balanced-paren aware and recursive:
  - `Vec<i32>` -> `Vec<i32>`
  - `Vec(Vec<i32>)` -> `Vec<Vec<i32>>`
  - `Vec(Fn1(&[u8]) void)` -> `Vec<Fn1<&[u8]> void>`
  - comments are converted too; strings are preserved verbatim.
Skips `::Name(...)` (Rust path, e.g. Value::Int) and `.Name(...)` (method call).
"""
import sys

# H generic type constructors actually used across examples/docs.
GENERICS = {
    "Vec", "Map", "Table", "Pair", "PairPair", "Future", "Thread",
    "Fn1", "FnN", "IIterable", "ArrayLen", "OneToOne", "OneToMany",
    "ManyToOne", "ManyToMany", "LinkedList", "Opt", "List",
}


def skip_string(s, i, n):
    """s[i] is a quote. Return index just past the closing quote."""
    q = s[i]
    i += 1
    while i < n:
        if s[i] == "\\" and i + 1 < n:
            i += 2
            continue
        if s[i] == q:
            return i + 1
        i += 1
    return i


def find_matching_paren(s, i, n):
    """s[i] == '('. Return index of the matching ')' (skipping strings)."""
    depth = 1
    j = i + 1
    while j < n:
        c = s[j]
        if c in ('"', "'"):
            j = skip_string(s, j, n)
            continue
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return j
        j += 1
    return -1


def convert(s, in_comment=False):
    out = []
    i = 0
    n = len(s)
    while i < n:
        ch = s[i]
        # string literal -> copy verbatim (parens inside not counted)
        if ch in ('"', "'"):
            j = skip_string(s, i, n)
            out.append(s[i:j])
            i = j
            continue
        # line comment -> convert inner generics too (unless already in one)
        if not in_comment and ch == "/" and i + 1 < n and s[i + 1] == "/":
            j = s.find("\n", i)
            if j == -1:
                out.append("//")
                out.append(convert(s[i + 2:], in_comment=True))
                break
            out.append("//")
            out.append(convert(s[i + 2:j], in_comment=True))
            out.append("\n")
            i = j + 1
            continue
        # block comment -> convert inner generics too (URLs safe since
        # `//` inside is not treated as a nested comment when in_comment)
        if not in_comment and ch == "/" and i + 1 < n and s[i + 1] == "*":
            j = s.find("*/", i + 2)
            if j == -1:
                out.append("/*")
                out.append(convert(s[i + 2:], in_comment=True))
                break
            out.append("/*")
            out.append(convert(s[i + 2:j], in_comment=True))
            out.append("*/")
            i = j + 2
            continue
        # identifier
        if ch.isalpha() or ch == "_":
            j = i
            while j < n and (s[j].isalnum() or s[j] == "_"):
                j += 1
            name = s[i:j]
            # lookahead: next non-space char
            k = j
            while k < n and s[k] in " \t":
                k += 1
            # preceding char (skip whitespace back); skip Rust `::` paths and
            # method calls (`.name(`), but NOT `: Name(` type annotations.
            p = i - 1
            while p >= 0 and s[p] in " \t":
                p -= 1
            prev = s[p] if p >= 0 else ""
            prev2 = s[p - 1] if p >= 1 else ""
            is_rust_path = prev == ":" and prev2 == ":"
            if (k < n and s[k] == "(" and name in GENERICS
                    and not is_rust_path and prev != "."):
                end = find_matching_paren(s, k, n)
                if end == -1:
                    out.append(name)
                    i = j
                    continue
                inner = convert(s[k + 1:end])
                out.append(name)
                out.append("<")
                out.append(inner)
                out.append(">")
                i = end + 1
                continue
            out.append(name)
            i = j
            continue
        out.append(ch)
        i += 1
    return "".join(out)


def main():
    for path in sys.argv[1:]:
        with open(path, "r", encoding="utf-8") as f:
            data = f.read()
        new = convert(data)
        if new != data:
            with open(path, "w", encoding="utf-8", newline="") as f:
                f.write(new)
            print("changed:", path)


if __name__ == "__main__":
    main()
