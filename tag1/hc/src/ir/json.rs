use super::*;

pub(crate) fn parse_json_value_ir(ctx: &mut Ctx, s: &str) -> R<(IrValue, usize)> {
    let s = s.trim_start();
    match s.as_bytes().first().copied() {
        Some(b'{') => parse_json_object_ir(ctx, s),
        Some(b'[') => parse_json_array_ir(ctx, s),
        Some(b'"') => parse_json_string_ir(s),
        Some(b't') if s.starts_with("true") => Ok((IrValue::Bool(true), 4)),
        Some(b'f') if s.starts_with("false") => Ok((IrValue::Bool(false), 5)),
        Some(b'n') if s.starts_with("null") => Ok((IrValue::Opt(None), 4)),
        Some(c) if c == b'-' || c.is_ascii_digit() => parse_json_number_ir(s),
        _ => Err(IrError::msg("InvalidJson", "unexpected token")),
    }
}

pub(crate) fn parse_json_object_ir(ctx: &mut Ctx, s: &str) -> R<(IrValue, usize)> {
    let b = s.as_bytes();
    let mut fields: HashMap<String, usize> = HashMap::new();
    let mut pos = 1usize;
    loop {
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= b.len() {
            return Err(IrError::msg("InvalidJson", "unterminated object"));
        }
        if b[pos] == b'}' {
            let class = IrValue::Class(ctx.alloc(Cell::Class {
                name: "Map".into(),
                fields,
            }));
            return Ok((class, pos + 1));
        }
        if b[pos] != b'"' {
            return Err(IrError::msg("InvalidJson", "expected string key"));
        }
        let (key, klen) = parse_json_string_ir(&s[pos..])?;
        pos += klen;
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= b.len() || b[pos] != b':' {
            return Err(IrError::msg("InvalidJson", "expected ':'"));
        }
        pos += 1;
        let (val, vlen) = parse_json_value_ir(ctx, &s[pos..])?;
        pos += vlen;
        if let IrValue::Str(ks) = key {
            fields.insert(
                String::from_utf8_lossy(&ks).to_string(),
                ctx.alloc(Cell::Value(val)),
            );
        }
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        match b.get(pos).copied() {
            Some(b',') => pos += 1,
            Some(b'}') => {
                let class = IrValue::Class(ctx.alloc(Cell::Class {
                    name: "Map".into(),
                    fields,
                }));
                return Ok((class, pos + 1));
            }
            _ => return Err(IrError::msg("InvalidJson", "expected ',' or '}'")),
        }
    }
}

pub(crate) fn parse_json_array_ir(ctx: &mut Ctx, s: &str) -> R<(IrValue, usize)> {
    let b = s.as_bytes();
    let mut items: Vec<IrValue> = Vec::new();
    let mut pos = 1usize;
    loop {
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= b.len() {
            return Err(IrError::msg("InvalidJson", "unterminated array"));
        }
        if b[pos] == b']' {
            return Ok((make_arr(ctx, items), pos + 1));
        }
        let (val, vlen) = parse_json_value_ir(ctx, &s[pos..])?;
        pos += vlen;
        items.push(val);
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        match b.get(pos).copied() {
            Some(b',') => pos += 1,
            Some(b']') => return Ok((make_arr(ctx, items), pos + 1)),
            _ => return Err(IrError::msg("InvalidJson", "expected ',' or ']'")),
        }
    }
}

pub(crate) fn parse_json_string_ir(s: &str) -> R<(IrValue, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 1usize;
    while i < b.len() {
        match b[i] {
            b'"' => return Ok((IrValue::Str(out), i + 1)),
            b'\\' => {
                i += 1;
                if i >= b.len() {
                    return Err(IrError::msg("InvalidJson", "bad escape"));
                }
                match b[i] {
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    _ => return Err(IrError::msg("InvalidJson", "unknown escape")),
                }
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Err(IrError::msg("InvalidJson", "unterminated string"))
}

pub(crate) fn parse_json_number_ir(s: &str) -> R<(IrValue, usize)> {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() && (b[i].is_ascii_digit() || matches!(b[i], b'-' | b'+' | b'.' | b'e' | b'E'))
    {
        i += 1;
    }
    let text = &s[..i];
    let v = if text.contains('.') || text.contains('e') || text.contains('E') {
        IrValue::Float(
            text.parse::<f64>()
                .map_err(|_| IrError::msg("InvalidJson", "bad number"))?,
        )
    } else {
        match text.parse::<i128>() {
            Ok(n) => IrValue::Int(n),
            Err(_) => IrValue::Float(
                text.parse::<f64>()
                    .map_err(|_| IrError::msg("InvalidJson", "bad number"))?,
            ),
        }
    };
    Ok((v, i))
}

pub(crate) fn parse_json_obj_ir(ctx: &mut Ctx, s: &str) -> R<HashMap<String, IrValue>> {
    let (v, _) = parse_json_value_ir(ctx, s)?;
    match v {
        IrValue::Class(c) => match &ctx.cells[c] {
            Cell::Class { fields, .. } => Ok(fields
                .iter()
                .map(|(k, vc)| (k.clone(), ctx.cell_value(*vc).clone()))
                .collect()),
            _ => Ok(HashMap::new()),
        },
        _ => Ok(HashMap::new()),
    }
}
