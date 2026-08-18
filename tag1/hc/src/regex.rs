//! 正则引擎（G5 io.text）——纯函数共享层（ADR-0004 语义唯一源）
//!
//! 供 `hc-rt` 树走解释器（interp）与 `hc` IR 后端（ir.rs）共用，消除重复实现。
//! 不依赖任一运行时值模型：操作对象为 `&[u8]` / `usize`，匹配结果 = 位置区间。
//! 支持子集：字面量 / `.`（任意字节）/ `[...]`（类：范围、`^` 取反、`\d` `\w` `\s`
//! 转义）/ `(...)` 分组 / `a*` `a+` `a?` `a{n}` `a{n,}` `a{n,m}`（贪婪）/ `a|b`
//! 交替 / `^` `$` 锚定 / `\n` `\t` `\r` `\xNN` 及转义元字符。非法模式（未闭合
//! 括号/类、裸量词、降序范围）→ 解析失败（io.text 方法 → error.InvalidFormat）。

use std::collections::HashMap;
use std::rc::Rc;

/// 正则 AST 节点（平坦数组 + 子节点索引；`RegexAst` 持有全部节点，匹配按索引递归）。
#[derive(Debug, Clone)]
pub enum RegexNode {
    /// 单字节字面量
    Char(u8),
    /// `.`：任意字节
    Any,
    /// 字符类：`(lo, hi)` 闭区间列表 + 取反（`[^...]`）
    Class(Vec<(u8, u8)>, bool),
    /// 串联（abc）：子节点索引，按序消费
    Concat(Vec<usize>),
    /// 重复：子节点索引 + 次数域（min..=max，None 无限）
    Repeat {
        child: usize,
        min: usize,
        max: Option<usize>,
    },
    /// 交替（a|b）：分支子节点索引（任一分支成功即匹配）
    Alt(Vec<usize>),
    /// `^`：仅位置 0 匹配
    Start,
    /// `$`：仅末尾匹配
    End,
}

/// 编译后的正则（平坦节点数组 + 根索引）
pub struct RegexAst {
    nodes: Vec<RegexNode>,
    root: usize,
}

/// 数字字面量（量词 `{n,m}` 用）
fn parse_digits(p: &[u8], i: &mut usize) -> Option<usize> {
    let start = *i;
    while *i < p.len() && p[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i == start {
        return None;
    }
    let mut n = 0usize;
    for &b in &p[start..*i] {
        n = n * 10 + (b - b'0') as usize;
    }
    Some(n)
}

/// 交替层：`parse_concat ('|' parse_concat)*`——`|` 优先级最低
fn parse_alt(p: &[u8], i: &mut usize, nodes: &mut Vec<RegexNode>) -> Option<usize> {
    let mut branches = vec![parse_concat(p, i, nodes)?];
    while *i < p.len() && p[*i] == b'|' {
        *i += 1;
        branches.push(parse_concat(p, i, nodes)?);
    }
    if branches.len() == 1 {
        Some(branches[0])
    } else {
        nodes.push(RegexNode::Alt(branches));
        Some(nodes.len() - 1)
    }
}

/// 串联层：`parse_repeat*`（遇 `|` / `)` / 结束即止；空序列 = 匹配空串）
fn parse_concat(p: &[u8], i: &mut usize, nodes: &mut Vec<RegexNode>) -> Option<usize> {
    let mut kids = Vec::new();
    while *i < p.len() && p[*i] != b'|' && p[*i] != b')' {
        kids.push(parse_repeat(p, i, nodes)?);
    }
    if kids.is_empty() {
        nodes.push(RegexNode::Concat(Vec::new()));
        Some(nodes.len() - 1)
    } else if kids.len() == 1 {
        Some(kids[0])
    } else {
        nodes.push(RegexNode::Concat(kids));
        Some(nodes.len() - 1)
    }
}

/// 重复层：`parse_atom ('*' | '+' | '?' | '{n}' | '{n,}' | '{n,m}')?`
fn parse_repeat(p: &[u8], i: &mut usize, nodes: &mut Vec<RegexNode>) -> Option<usize> {
    let atom = parse_atom(p, i, nodes)?;
    if *i >= p.len() {
        return Some(atom);
    }
    let (min, max) = match p[*i] {
        b'*' => {
            *i += 1;
            (0, None)
        }
        b'+' => {
            *i += 1;
            (1, None)
        }
        b'?' => {
            *i += 1;
            (0, Some(1))
        }
        b'{' => {
            // 试解析 `{n}` / `{n,}` / `{n,m}`；语法不匹配 → 字面 `{`（回退不消费）
            let save = *i;
            *i += 1;
            let lo = match parse_digits(p, i) {
                Some(n) => n,
                None => {
                    *i = save;
                    return Some(atom);
                }
            };
            if *i < p.len() && p[*i] == b',' {
                *i += 1;
                if *i < p.len() && p[*i] == b'}' {
                    *i += 1;
                    (lo, None)
                } else if let Some(m) = parse_digits(p, i) {
                    if *i < p.len() && p[*i] == b'}' {
                        *i += 1;
                        if lo > m {
                            return None; // {3,2} 非法
                        }
                        (lo, Some(m))
                    } else {
                        *i = save;
                        return Some(atom);
                    }
                } else {
                    *i = save;
                    return Some(atom);
                }
            } else if *i < p.len() && p[*i] == b'}' {
                *i += 1;
                (lo, Some(lo))
            } else {
                *i = save;
                return Some(atom);
            }
        }
        _ => return Some(atom),
    };
    nodes.push(RegexNode::Repeat {
        child: atom,
        min,
        max,
    });
    Some(nodes.len() - 1)
}

/// 原子层：`(` 分组 / `^` / `$` / `.` / `[...]` 类 / `\x` 转义 / 字面量
fn parse_atom(p: &[u8], i: &mut usize, nodes: &mut Vec<RegexNode>) -> Option<usize> {
    if *i >= p.len() {
        return None;
    }
    match p[*i] {
        b'(' => {
            *i += 1;
            let inner = parse_alt(p, i, nodes)?;
            if *i < p.len() && p[*i] == b')' {
                *i += 1;
                Some(inner)
            } else {
                None // 未闭合分组
            }
        }
        b'|' | b')' => None,        // 交替/闭合不在 atom 位置
        b'*' | b'+' | b'?' => None, // 裸量词（无被修饰原子）→ 非法
        b'^' => {
            *i += 1;
            nodes.push(RegexNode::Start);
            Some(nodes.len() - 1)
        }
        b'$' => {
            *i += 1;
            nodes.push(RegexNode::End);
            Some(nodes.len() - 1)
        }
        b'.' => {
            *i += 1;
            nodes.push(RegexNode::Any);
            Some(nodes.len() - 1)
        }
        b'[' => parse_class(p, i, nodes),
        b'\\' => parse_escape(p, i, nodes),
        c => {
            *i += 1;
            nodes.push(RegexNode::Char(c));
            Some(nodes.len() - 1)
        }
    }
}

/// 字符类解析：`[`（`^` 取反）…`]`——字面、范围 `a-z`、转义 `\d` `\w` `\s` 及取反、
/// `\n` `\t` `\r` `\xNN`、转义元字符；`]` 位于首字符时为字面（`[]]` 含 `]`）。
fn parse_class(p: &[u8], i: &mut usize, nodes: &mut Vec<RegexNode>) -> Option<usize> {
    *i += 1; // '['
    let negated = *i < p.len() && p[*i] == b'^';
    if negated {
        *i += 1;
    }
    let mut ranges: Vec<(u8, u8)> = Vec::new();
    let mut first = true;
    loop {
        if *i >= p.len() {
            return None; // 未闭合类
        }
        let b = p[*i];
        if b == b']' && !first {
            *i += 1;
            break;
        }
        first = false;
        if b == b'\\' {
            // 类内转义：类速记展开为范围；单字节转义按字面（支持范围端点）
            match p.get(*i + 1) {
                Some(b'd') => {
                    *i += 2;
                    ranges.push((b'0', b'9'));
                }
                Some(b'D') => {
                    *i += 2;
                    ranges.push((0, b'0' - 1));
                    ranges.push((b'9' + 1, 255));
                }
                Some(b'w') => {
                    *i += 2;
                    ranges.push((b'0', b'9'));
                    ranges.push((b'A', b'Z'));
                    ranges.push((b'a', b'z'));
                    ranges.push((b'_', b'_'));
                }
                Some(b'W') => {
                    *i += 2;
                    ranges.push((0, 47));
                    ranges.push((58, 64));
                    ranges.push((91, 94));
                    ranges.push((96, 96));
                    ranges.push((123, 255));
                }
                Some(b's') => {
                    *i += 2;
                    ranges.push((0x09, 0x0d)); // \t \n \v \f \r
                    ranges.push((0x20, 0x20)); // 空格
                }
                Some(b'S') => {
                    *i += 2;
                    ranges.push((0, 8));
                    ranges.push((14, 31));
                    ranges.push((33, 255));
                }
                Some(_) => {
                    let lo = parse_escape_char(p, i)?;
                    if *i + 1 < p.len() && p[*i] == b'-' && p[*i + 1] != b']' {
                        *i += 1;
                        let hi = if p[*i] == b'\\' {
                            parse_escape_char(p, i)?
                        } else {
                            let h = p[*i];
                            *i += 1;
                            h
                        };
                        if lo > hi {
                            return None;
                        }
                        ranges.push((lo, hi));
                    } else {
                        ranges.push((lo, lo));
                    }
                }
                None => return None,
            }
            continue;
        }
        // 字面量（可能构成范围 `lo-hi`）
        let lo = b;
        *i += 1;
        if *i + 1 < p.len() && p[*i] == b'-' && p[*i + 1] != b']' {
            *i += 1; // '-'
            let hi = if p[*i] == b'\\' {
                parse_escape_char(p, i)?
            } else {
                let h = p[*i];
                *i += 1;
                h
            };
            if lo > hi {
                return None; // 非法降序范围
            }
            ranges.push((lo, hi));
        } else {
            ranges.push((lo, lo));
        }
    }
    nodes.push(RegexNode::Class(ranges, negated));
    Some(nodes.len() - 1)
}

/// 转义单字节（类内 / 范围端点）：`\n` `\t` `\r` `\0` `\xNN`、其余字面化
/// （含元字符 `\.` `\*` 等；未知转义按字面放宽）。调用点保证 `p[*i] == b'\\'`。
fn parse_escape_char(p: &[u8], i: &mut usize) -> Option<u8> {
    let c = *p.get(*i + 1)?;
    *i += 2;
    match c {
        b'n' => Some(b'\n'),
        b't' => Some(b'\t'),
        b'r' => Some(b'\r'),
        b'0' => Some(b'\0'),
        b'x' => {
            let hex = std::str::from_utf8(p.get(*i..*i + 2)?).ok()?;
            let v = u8::from_str_radix(hex, 16).ok()?;
            *i += 2;
            Some(v)
        }
        c => Some(c),
    }
}

/// 类外转义：类速记展开为 Class 节点；`\n` `\t` `\r` `\0` `\xNN` 为 Char；元字符字面化。
fn parse_escape(p: &[u8], i: &mut usize, nodes: &mut Vec<RegexNode>) -> Option<usize> {
    let c = *p.get(*i + 1)?;
    *i += 2;
    let node = match c {
        b'd' => RegexNode::Class(vec![(b'0', b'9')], false),
        b'D' => RegexNode::Class(vec![(0, b'0' - 1), (b'9' + 1, 255)], false),
        b'w' => RegexNode::Class(
            vec![(b'0', b'9'), (b'A', b'Z'), (b'a', b'z'), (b'_', b'_')],
            false,
        ),
        b'W' => RegexNode::Class(
            vec![(0, 47), (58, 64), (91, 94), (96, 96), (123, 255)],
            false,
        ),
        b's' => RegexNode::Class(vec![(0x09, 0x0d), (0x20, 0x20)], false),
        b'S' => RegexNode::Class(vec![(0, 8), (14, 31), (33, 255)], false),
        b'n' => RegexNode::Char(b'\n'),
        b't' => RegexNode::Char(b'\t'),
        b'r' => RegexNode::Char(b'\r'),
        b'0' => RegexNode::Char(b'\0'),
        b'x' => {
            let hex = std::str::from_utf8(p.get(*i..*i + 2)?).ok()?;
            let v = u8::from_str_radix(hex, 16).ok()?;
            *i += 2;
            RegexNode::Char(v)
        }
        c => RegexNode::Char(c), // 转义元字符 → 字面
    };
    nodes.push(node);
    Some(nodes.len() - 1)
}

/// 编译正则：完整消费 → Some(Ast)；未消费（未闭合分组/裸 `)`）→ None。
pub fn parse_regex(pattern: &[u8]) -> Option<RegexAst> {
    let mut nodes = Vec::new();
    let mut i = 0usize;
    let root = parse_alt(pattern, &mut i, &mut nodes)?;
    if i != pattern.len() {
        return None;
    }
    Some(RegexAst { nodes, root })
}

/// 匹配器：记忆化集合回溯。`ends(node, pos)` → 该节点在 pos 处匹配的全部「结束位置」
/// 集合（Rc 共享，键 (节点索引, 位置)）——集合语义保证回溯完备且不重算；Repeat 闭包
/// 收敛（每次新增结束位置必前进；零宽重复不新增 → 不动点终止），无灾难性回溯。
pub struct RegexMatcher<'a> {
    ast: &'a RegexAst,
    text: &'a [u8],
    memo: HashMap<(usize, usize), Rc<std::collections::HashSet<usize>>>,
}

impl<'a> RegexMatcher<'a> {
    pub fn new(ast: &'a RegexAst, text: &'a [u8]) -> Self {
        RegexMatcher {
            ast,
            text,
            memo: HashMap::new(),
        }
    }

    fn ends(&mut self, node: usize, pos: usize) -> Rc<std::collections::HashSet<usize>> {
        if let Some(c) = self.memo.get(&(node, pos)) {
            return c.clone();
        }
        let len = self.text.len();
        let mut set = std::collections::HashSet::new();
        match &self.ast.nodes[node] {
            RegexNode::Char(c) => {
                if pos < len && self.text[pos] == *c {
                    set.insert(pos + 1);
                }
            }
            RegexNode::Any => {
                if pos < len {
                    set.insert(pos + 1);
                }
            }
            RegexNode::Class(ranges, negated) => {
                if pos < len {
                    let b = self.text[pos];
                    let in_cls = ranges.iter().any(|&(lo, hi)| b >= lo && b <= hi);
                    if in_cls != *negated {
                        set.insert(pos + 1);
                    }
                }
            }
            RegexNode::Concat(children) => {
                let mut cur: std::collections::HashSet<usize> =
                    std::collections::HashSet::from([pos]);
                for &child in children {
                    let mut next = std::collections::HashSet::new();
                    for &e in &cur {
                        next.extend(self.ends(child, e).iter().copied());
                    }
                    cur = next;
                    if cur.is_empty() {
                        break;
                    }
                }
                set = cur;
            }
            RegexNode::Repeat { child, min, max } => {
                // 恰好 min 次后的位置（扩展起点）
                let mut frontier: std::collections::HashSet<usize> =
                    std::collections::HashSet::from([pos]);
                for _ in 0..*min {
                    let mut next = std::collections::HashSet::new();
                    for &e in &frontier {
                        next.extend(self.ends(*child, e).iter().copied());
                    }
                    frontier = next;
                    if frontier.is_empty() {
                        break;
                    }
                }
                // 计数在 [min, max]（或 [min, ∞)）内的全部结束位置
                set.extend(frontier.iter().copied());
                if max.is_none() || max.unwrap() > *min {
                    let mut rounds = *min;
                    loop {
                        if let Some(m) = max {
                            if rounds >= *m {
                                break;
                            }
                        }
                        let mut next = std::collections::HashSet::new();
                        for &e in &frontier {
                            next.extend(self.ends(*child, e).iter().copied());
                        }
                        if next.is_empty() {
                            break;
                        }
                        let mut grew = false;
                        for &e in &next {
                            if set.insert(e) {
                                grew = true;
                            }
                        }
                        if !grew {
                            break; // 不动点（零宽重复等）
                        }
                        frontier = next;
                        rounds += 1;
                    }
                }
            }
            RegexNode::Alt(branches) => {
                for &b in branches {
                    set.extend(self.ends(b, pos).iter().copied());
                }
            }
            RegexNode::Start => {
                if pos == 0 {
                    set.insert(0);
                }
            }
            RegexNode::End => {
                if pos == len {
                    set.insert(len);
                }
            }
        }
        let rc = Rc::new(set);
        self.memo.insert((node, pos), rc.clone());
        rc
    }

    /// 自 `start` 起首个匹配区间（左起最短起点 + 最长结束——replace/split 按最长段
    /// 消费）；无匹配 → None。空匹配返回 (s, s)。
    pub fn find_at(&mut self, start: usize) -> Option<(usize, usize)> {
        let len = self.text.len();
        for s in start..=len {
            let es = self.ends(self.ast.root, s);
            if !es.is_empty() {
                let e = es.iter().copied().max().unwrap();
                return Some((s, e));
            }
        }
        None
    }
}
