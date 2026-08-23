use super::*;

impl Interp {
    // ---------- 序列化内建辅助（M4.4；连续类型 byte 化、class json 化） ----------

    /// 标量声明宽度字节数（自然对齐 = 宽度）
    pub(crate) fn scalar_size(n: &str) -> usize {
        match n {
            "i8" | "u8" | "bool" => 1,
            "i16" | "u16" => 2,
            "i32" | "u32" | "f32" => 4,
            _ => 8, // i64/u64/isize/usize/f64/f16/f128
        }
    }

    /// M4.3：连续 class 布局——字段 (名, 偏移, 大小) 列表 + 总大小
    /// （与 to_bytes 直映射一致：自然对齐 + 字段间填充 + 尾部圆整；
    ///   [pad] = 紧凑对齐 1；[align(T)] = 尾部圆整到 T 的对齐值）
    pub(crate) fn continuous_layout(
        &self,
        ty: &str,
    ) -> Option<(Vec<(String, usize, usize)>, usize)> {
        let (fdecls, traits) = match self.types.get(ty) {
            Some(TypeDef::Class { fields, traits, .. }) => (fields, traits),
            _ => return None,
        };
        if !traits.iter().any(|t| matches!(t, Trait::Continuous)) {
            return None;
        }
        let packed = traits.iter().any(|t| matches!(t, Trait::Pad));
        let mut layout: Vec<(String, usize, usize)> = Vec::new();
        let mut offset = 0usize;
        let mut max_align = 1usize;
        for fd in fdecls {
            let size = match self.field_serialized_size(&fd.ty) {
                Some(s) => s,
                None => continue, // 非序列化字段不占字节
            };
            let align = if packed {
                1
            } else {
                self.field_align(&fd.ty, size)
            };
            max_align = max_align.max(align);
            while offset % align != 0 {
                offset += 1;
            }
            layout.push((fd.name.clone(), offset, size));
            offset += size;
        }
        // 尾部圆整：[pad] → 1；[align(T)] → T 对齐值；否则最大字段对齐
        let tail_align = if packed {
            1
        } else if let Some(Trait::Align(a)) = traits.iter().find(|t| matches!(t, Trait::Align(_))) {
            Self::scalar_size(a)
        } else {
            max_align
        };
        while offset % tail_align != 0 {
            offset += 1;
        }
        Some((layout, offset))
    }

    /// 字段自然对齐值（连续布局用；标量 = 宽度；嵌套连续 = 其自身 alignOf；其余 1）
    pub(crate) fn field_align(&self, t: &Type, size: usize) -> usize {
        match t.strip() {
            Type::Named(n, _) if self.is_nested_continuous(n) => {
                self.continuous_align(n).unwrap_or(1)
            }
            Type::Named(n, _) if Self::scalar_size(n) == size => size,
            _ => 1,
        }
    }

    /// 连续 class 的对齐值：pad → 1；align(T) → T 对齐值；否则最大字段对齐
    pub(crate) fn continuous_align(&self, ty: &str) -> Option<usize> {
        let (fdecls, traits) = match self.types.get(ty) {
            Some(TypeDef::Class { fields, traits, .. }) => (fields, traits),
            _ => return None,
        };
        if !traits.iter().any(|t| matches!(t, Trait::Continuous)) {
            return None;
        }
        if traits.iter().any(|t| matches!(t, Trait::Pad)) {
            return Some(1);
        }
        if let Some(Trait::Align(a)) = traits.iter().find(|t| matches!(t, Trait::Align(_))) {
            return Some(Self::scalar_size(a));
        }
        let mut max_a = 1usize;
        for fd in fdecls {
            if let Some(s) = self.field_serialized_size(&fd.ty) {
                max_a = max_a.max(self.field_align(&fd.ty, s));
            }
        }
        Some(max_a)
    }

    pub(crate) fn is_nested_continuous(&self, n: &str) -> bool {
        matches!(
            self.types.get(n),
            Some(TypeDef::Class { traits, .. })
                if traits.iter().any(|t| matches!(t, Trait::Continuous))
        )
    }

    /// 字段序列化字节大小（连续布局用；标量 / 嵌套连续 / 元组）
    pub(crate) fn field_serialized_size(&self, t: &Type) -> Option<usize> {
        match t.strip() {
            Type::Named(n, _) => {
                if Self::is_scalar_name(n) {
                    Some(Self::scalar_size(n))
                } else if self.is_nested_continuous(n) {
                    self.continuous_layout(n).map(|(_, size)| size)
                } else {
                    None
                }
            }
            Type::Tuple(ts) => {
                let mut s = 0usize;
                for x in ts {
                    s += self.field_serialized_size(x)?;
                }
                Some(s)
            }
            _ => None,
        }
    }

    pub(crate) fn is_scalar_name(n: &str) -> bool {
        matches!(
            n,
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f16"
                | "f32"
                | "f64"
                | "f128"
                | "bool"
        )
    }

    /// M4.3：@intCast 目标宽度范围（Debug 溢出检查）
    pub(crate) fn int_width_bounds(n: &str) -> Option<(i128, i128)> {
        match n {
            "i8" => Some((i8::MIN as i128, i8::MAX as i128)),
            "i16" => Some((i16::MIN as i128, i16::MAX as i128)),
            "i32" => Some((i32::MIN as i128, i32::MAX as i128)),
            "i64" => Some((i64::MIN as i128, i64::MAX as i128)),
            "i128" => Some((i128::MIN, i128::MAX)),
            "isize" => Some((isize::MIN as i128, isize::MAX as i128)),
            "u8" => Some((0, u8::MAX as i128)),
            "u16" => Some((0, u16::MAX as i128)),
            "u32" => Some((0, u32::MAX as i128)),
            "u64" => Some((0, u64::MAX as i128)),
            "u128" => Some((0, u128::MAX as i128)),
            "usize" => Some((0, usize::MAX as i128)),
            _ => None,
        }
    }

    /// M4.3：@sizeOf(T) 类型字节大小
    pub(crate) fn type_size_of(&self, ty: &str) -> Option<usize> {
        match ty {
            "i8" | "u8" | "bool" => Some(1),
            "i16" | "u16" | "f16" => Some(2),
            "i32" | "u32" | "f32" => Some(4),
            "i128" | "u128" | "f128" => Some(16),
            "i64" | "u64" | "isize" | "usize" | "f64" => Some(8),
            // 引用类型（String/集合/Table/堆上 class/指针/切片）= 指针宽
            "String" | "Vec" | "Map" | "Deque" | "Table" | "Allocator" => Some(8),
            _ => match self.types.get(ty) {
                Some(TypeDef::Class { traits, .. })
                    if traits.iter().any(|t| matches!(t, Trait::Continuous)) =>
                {
                    self.continuous_layout(ty).map(|(_, size)| size)
                }
                Some(TypeDef::Class { .. }) => Some(8), // 堆上 = 指针
                Some(TypeDef::Enum { variants }) => {
                    // 纯常量枚举 1 字节；带负载 = 最大负载大小（简化）
                    if variants.iter().all(|v| v.payload.is_none()) {
                        Some(1)
                    } else {
                        let mut max_s = 1usize;
                        for v in variants {
                            if let Some(p) = &v.payload {
                                if let Some(s) = self.field_serialized_size(p) {
                                    max_s = max_s.max(s);
                                }
                            }
                        }
                        Some(max_s)
                    }
                }
                Some(TypeDef::Interface { .. }) => Some(8),
                // K1 无标签 union（ADR-0014）：size = 最大字段宽度
                Some(TypeDef::Union { fields }) => {
                    let mut max_s = 0usize;
                    for fd in fields {
                        if let Type::Named(n, _) = fd.ty.strip() {
                            if let Some(s) = Self::union_scalar_width(n) {
                                max_s = max_s.max(s);
                            }
                        }
                    }
                    Some(max_s)
                }
                _ => None,
            },
        }
    }

    /// class 实例 → 字节（M4.4 直映射；连续类型尊重 pad/align/嵌套/元组，堆上走自然对齐标量近似）
    pub(crate) fn class_to_bytes(
        &self,
        ty: &str,
        fields: &HashMap<String, Value>,
    ) -> Result<Vec<u8>> {
        let Some(TypeDef::Class { fields: fdecls, .. }) = self.types.get(ty) else {
            return Err(RtError::msg("UnknownType", format!("unknown type `{ty}`")));
        };
        // 连续类型：布局驱动（与 @offsetOf/@sizeOf 同源）
        if let Some((layout, total)) = self.continuous_layout(ty) {
            let mut out = vec![0u8; total];
            let offmap: HashMap<&str, (usize, usize)> = layout
                .iter()
                .map(|(n, o, s)| (n.as_str(), (*o, *s)))
                .collect();
            for fd in fdecls {
                let Some(&(off, size)) = offmap.get(fd.name.as_str()) else {
                    continue;
                };
                let v = fields.get(&fd.name).cloned().unwrap_or(Value::Void);
                let v = self.deref_value(v);
                self.write_field_bytes(&mut out, off, size, &fd.ty, &v)?;
            }
            return Ok(out);
        }
        // 堆上 class：自然对齐标量直映射（tag1 近似；非标量字段跳过——堆类型请用 to_json）
        let mut out = Vec::new();
        let mut offset = 0usize;
        let mut max_align = 1usize;
        for fd in fdecls {
            let n = match fd.ty.strip() {
                Type::Named(n, _) if Self::is_scalar_name(n) => n.clone(),
                _ => continue,
            };
            let v = fields.get(&fd.name).cloned().unwrap_or(Value::Void);
            let v = self.deref_value(v);
            if matches!(v, Value::Void) {
                continue;
            }
            let size = Self::scalar_size(&n);
            let align = size;
            max_align = max_align.max(align);
            while offset % align != 0 {
                out.push(0);
                offset += 1;
            }
            let mut buf = vec![0u8; size];
            self.write_scalar(&mut buf, &n, &v);
            out.extend_from_slice(&buf);
            offset += size;
        }
        while offset % max_align != 0 {
            out.push(0);
            offset += 1;
        }
        Ok(out)
    }

    /// bytes → class 实例（连续类型按布局解析；堆上按自然对齐标量近似）
    pub(crate) fn class_from_bytes(&self, ty: &str, bytes: &[u8]) -> Result<Value> {
        let Some(TypeDef::Class { fields: fdecls, .. }) = self.types.get(ty) else {
            return Err(RtError::msg("UnknownType", format!("unknown type `{ty}`")));
        };
        // 连续类型：布局驱动（与 @offsetOf/@sizeOf 同源）
        if let Some((layout, _total)) = self.continuous_layout(ty) {
            let mut f = HashMap::new();
            let offmap: HashMap<&str, (usize, usize)> = layout
                .iter()
                .map(|(n, o, s)| (n.as_str(), (*o, *s)))
                .collect();
            for fd in fdecls {
                let v = match offmap.get(fd.name.as_str()) {
                    Some(&(off, size)) => self.read_field_bytes(bytes, off, size, &fd.ty)?,
                    None => Value::Void,
                };
                f.insert(fd.name.clone(), v);
            }
            return Ok(Value::class(ty, f));
        }
        // 堆上 class：自然对齐标量解析（tag1 近似；非标量字段置 Void）
        let mut pos = 0usize;
        let mut f = HashMap::new();
        for fd in fdecls {
            let n = match fd.ty.strip() {
                Type::Named(n, _) if Self::is_scalar_name(n) => n.clone(),
                _ => {
                    f.insert(fd.name.clone(), Value::Void);
                    continue;
                }
            };
            let size = Self::scalar_size(&n);
            let align = size;
            while pos % align != 0 {
                pos += 1;
            }
            let v = self.read_scalar(bytes.get(pos..pos + size).unwrap_or(&[]), &n)?;
            pos += size;
            f.insert(fd.name.clone(), v);
        }
        Ok(Value::class(ty, f))
    }

    /// 连续字段 → 字节（写入 out[off..off+size]；标量 / 嵌套连续 / 元组递归）
    pub(crate) fn write_field_bytes(
        &self,
        out: &mut [u8],
        off: usize,
        size: usize,
        ty: &Type,
        v: &Value,
    ) -> Result<()> {
        match ty.strip() {
            Type::Named(n, _) if Self::is_scalar_name(n) => {
                self.write_scalar(&mut out[off..off + size], n, v);
            }
            Type::Named(n, _) if self.is_nested_continuous(n) => {
                let bytes = match v {
                    Value::Class(c) => self.class_to_bytes(n, &c.borrow().fields)?,
                    _ => vec![0u8; size], // 缺省字段零填充
                };
                let len = size.min(bytes.len());
                out[off..off + len].copy_from_slice(&bytes[..len]);
                if len < size {
                    out[off + len..off + size].fill(0);
                }
            }
            Type::Tuple(ts) => {
                let items = match v {
                    Value::Arr(a) => a.borrow().clone(),
                    Value::Vec(d) => d.borrow().items.borrow().clone(),
                    _ => Vec::new(),
                };
                let mut cur = off;
                for (i, t) in ts.iter().enumerate() {
                    let esz = self.field_serialized_size(t).unwrap_or(0);
                    let ev = items
                        .get(i)
                        .map(|c| self.deref_value(c.borrow().clone()))
                        .unwrap_or(Value::Void);
                    if esz > 0 {
                        self.write_field_bytes(out, cur, esz, t, &ev)?;
                    }
                    cur += esz;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 字节 → 连续字段值（标量 / 嵌套连续 / 元组递归）
    pub(crate) fn read_field_bytes(
        &self,
        bytes: &[u8],
        off: usize,
        size: usize,
        ty: &Type,
    ) -> Result<Value> {
        match ty.strip() {
            Type::Named(n, _) if Self::is_scalar_name(n) => {
                self.read_scalar(bytes.get(off..off + size).unwrap_or(&[]), n)
            }
            Type::Named(n, _) if self.is_nested_continuous(n) => {
                let sub = bytes
                    .get(off..off + size)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                self.class_from_bytes(n, sub)
            }
            Type::Tuple(ts) => {
                let mut items = Vec::new();
                let mut cur = off;
                for t in ts {
                    let esz = self.field_serialized_size(t).unwrap_or(0);
                    items.push(self.read_field_bytes(bytes, cur, esz, t)?);
                    cur += esz;
                }
                Ok(Value::arr(items))
            }
            _ => Ok(Value::Void),
        }
    }

    // ---------- K1 无标签 union（ADR-0014，2026-08-18）----------
    // 运行时形态 = `Value::Class` + `@union` 标记；字段值保持「写字段 → 字节重解释
    // 同步其余字段」的不变式，读任意字段即返回已同步值（C 风格内存双关）。

    /// union 字段默认值（零值占位：构造时未写字段 = Int(0)/Float(0.0)/Bool(false)）
    pub(crate) fn union_default_value(ty: &Type) -> Value {
        match ty.strip() {
            Type::Named(n, _) => match n.as_str() {
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
                | "u128" | "usize" => Value::Int(0),
                "f16" | "f32" | "f64" | "f128" => Value::Float(0.0),
                "bool" => Value::Bool(false),
                _ => Value::Void,
            },
            _ => Value::Void,
        }
    }

    /// union 标量字段字节宽度（i128/u128/f128 = 16 字节；余同 type_size_of）
    pub(crate) fn union_scalar_width(n: &str) -> Option<usize> {
        match n {
            "i8" | "u8" | "bool" => Some(1),
            "i16" | "u16" | "f16" => Some(2),
            "i32" | "u32" | "f32" => Some(4),
            "i64" | "u64" | "isize" | "usize" | "f64" => Some(8),
            "i128" | "u128" | "f128" => Some(16),
            _ => None,
        }
    }

    /// 标量值 → 小端字节（union 专用：i128/u128 全 16 字节，纠正 write_scalar 的 8 字节近似）
    pub(crate) fn union_write_scalar(out: &mut [u8], n: &str, v: &Value) {
        match (n, v) {
            ("i8" | "u8", Value::Int(i)) => out[0] = *i as u8,
            ("i16" | "u16", Value::Int(i)) => out[..2].copy_from_slice(&(*i as i16).to_le_bytes()),
            ("i32" | "u32", Value::Int(i)) => out[..4].copy_from_slice(&(*i as i32).to_le_bytes()),
            ("i64" | "u64" | "isize" | "usize", Value::Int(i)) => {
                out[..8].copy_from_slice(&(*i as i64).to_le_bytes())
            }
            ("i128" | "u128", Value::Int(i)) => out[..16].copy_from_slice(&i.to_le_bytes()),
            ("f32", Value::Float(f)) => out[..4].copy_from_slice(&(*f as f32).to_le_bytes()),
            ("f64" | "f16" | "f128", Value::Float(f)) => out[..8].copy_from_slice(&f.to_le_bytes()),
            ("bool", Value::Bool(b)) => out[0] = if *b { 1 } else { 0 },
            _ => {}
        }
    }

    /// 小端字节 → 标量值（union 专用）
    pub(crate) fn union_read_scalar(bytes: &[u8], n: &str) -> Result<Value> {
        match n {
            "i8" | "u8" => Ok(Value::Int(bytes.first().copied().unwrap_or(0) as i128)),
            "i16" | "u16" => {
                let b = bytes
                    .get(..2)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated union bytes"))?;
                Ok(Value::Int(i16::from_le_bytes(b.try_into().unwrap()) as i128))
            }
            "i32" | "u32" => {
                let b = bytes
                    .get(..4)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated union bytes"))?;
                Ok(Value::Int(i32::from_le_bytes(b.try_into().unwrap()) as i128))
            }
            "i64" | "u64" | "isize" | "usize" => {
                let b = bytes
                    .get(..8)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated union bytes"))?;
                Ok(Value::Int(i64::from_le_bytes(b.try_into().unwrap()) as i128))
            }
            "i128" | "u128" => {
                let b = bytes
                    .get(..16)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated union bytes"))?;
                Ok(Value::Int(i128::from_le_bytes(b.try_into().unwrap())))
            }
            "f32" => {
                let b = bytes
                    .get(..4)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated union bytes"))?;
                Ok(Value::Float(
                    f32::from_le_bytes(b.try_into().unwrap()) as f64
                ))
            }
            "f64" | "f16" | "f128" => {
                let b = bytes
                    .get(..8)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated union bytes"))?;
                Ok(Value::Float(f64::from_le_bytes(b.try_into().unwrap())))
            }
            "bool" => Ok(Value::Bool(bytes.first().copied().unwrap_or(0) != 0)),
            _ => Ok(Value::Void),
        }
    }

    /// K1 union 写字段同步：写 `written` 字段后，把该字段字节重解释为其余每个字段的类型。
    /// 维持「读任意字段 = 写后字节重解释」的 C 风格 union 语义（字段全标量，ADR-0014）。
    pub(crate) fn union_sync_fields(
        &self,
        c: &mut ClassData,
        written: &str,
        v: &Value,
    ) -> Result<()> {
        let Some(TypeDef::Union { fields }) = self.types.get(&c.name) else {
            return Err(RtError::msg(
                "TypeError",
                format!("`{}` 不是 union 类型", c.name),
            ));
        };
        let wty = fields
            .iter()
            .find(|f| f.name == written)
            .map(|f| f.ty.strip())
            .ok_or_else(|| {
                RtError::msg(
                    "NoField",
                    format!("union `{}` has no field `{written}`", c.name),
                )
            })?;
        let wname = match wty {
            Type::Named(n, _) => n.clone(),
            _ => return Err(RtError::msg("TypeError", "union 字段必须为标量类型")),
        };
        let width = Self::union_scalar_width(&wname)
            .ok_or_else(|| RtError::msg("TypeError", format!("字段 `{wname}` 无标量宽度")))?;
        let mut buf = vec![0u8; width];
        Self::union_write_scalar(&mut buf, &wname, v);
        for fd in fields {
            if fd.name == written {
                continue;
            }
            let fname = match fd.ty.strip() {
                Type::Named(n, _) => n.clone(),
                _ => continue,
            };
            let dv = Self::union_read_scalar(&buf, &fname)?;
            c.fields.insert(fd.name.clone(), dv);
        }
        Ok(())
    }

    /// 字段写入统一入口（K1：union 标记 → 写 + 同步重解释；class → 普通覆盖写）
    pub(crate) fn assign_class_field(
        &mut self,
        c: Rc<RefCell<ClassData>>,
        field: &str,
        v: Value,
    ) -> Result<()> {
        if c.borrow().fields.contains_key("@union") {
            let mut cd = c.borrow_mut();
            cd.fields.insert(field.to_string(), v.clone());
            self.union_sync_fields(&mut cd, field, &v)?;
        } else {
            c.borrow_mut().fields.insert(field.to_string(), v);
        }
        Ok(())
    }

    /// 标量值 → 小端字节（写入 out 前 size 字节；size 由 scalar_size 决定）
    pub(crate) fn write_scalar(&self, out: &mut [u8], n: &str, v: &Value) {
        match (n, v) {
            ("i8" | "u8", Value::Int(i)) => out[0] = *i as u8,
            ("i16" | "u16", Value::Int(i)) => out[..2].copy_from_slice(&(*i as i16).to_le_bytes()),
            ("i32" | "u32", Value::Int(i)) => out[..4].copy_from_slice(&(*i as i32).to_le_bytes()),
            ("i64" | "u64" | "isize" | "usize" | "i128" | "u128", Value::Int(i)) => {
                out[..8].copy_from_slice(&(*i as i64).to_le_bytes())
            }
            ("f32", Value::Float(f)) => out[..4].copy_from_slice(&(*f as f32).to_le_bytes()),
            ("f64" | "f16" | "f128", Value::Float(f)) => out[..8].copy_from_slice(&f.to_le_bytes()),
            ("bool", Value::Bool(b)) => out[0] = if *b { 1 } else { 0 },
            _ => {}
        }
    }

    /// 标量字节 → 值（小端；len 由 scalar_size 决定）
    pub(crate) fn read_scalar(&self, bytes: &[u8], n: &str) -> Result<Value> {
        match n {
            "i8" | "u8" => Ok(Value::Int(bytes.first().copied().unwrap_or(0) as i128)),
            "i16" | "u16" => {
                let b = bytes
                    .get(..2)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                Ok(Value::Int(i16::from_le_bytes(b.try_into().unwrap()) as i128))
            }
            "i32" | "u32" => {
                let b = bytes
                    .get(..4)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                Ok(Value::Int(i32::from_le_bytes(b.try_into().unwrap()) as i128))
            }
            "i64" | "u64" | "isize" | "usize" | "i128" | "u128" => {
                let b = bytes
                    .get(..8)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                Ok(Value::Int(i64::from_le_bytes(b.try_into().unwrap()) as i128))
            }
            "f32" => {
                let b = bytes
                    .get(..4)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                Ok(Value::Float(
                    f32::from_le_bytes(b.try_into().unwrap()) as f64
                ))
            }
            "f64" | "f16" | "f128" => {
                let b = bytes
                    .get(..8)
                    .ok_or_else(|| RtError::msg("InvalidBytes", "truncated byte data"))?;
                Ok(Value::Float(f64::from_le_bytes(b.try_into().unwrap())))
            }
            "bool" => Ok(Value::Bool(bytes.first().copied().unwrap_or(0) != 0)),
            _ => Ok(Value::Void),
        }
    }

    /// 任意值 → 字节（标量/嵌套；Int 在 i32 范围用 4 字节——i32 元素集合 12 字节）
    pub(crate) fn value_to_bytes(&self, v: &Value) -> Vec<u8> {
        match v {
            Value::Int(i) => {
                if *i >= i32::MIN as i128 && *i <= i32::MAX as i128 {
                    (*i as i32).to_le_bytes().to_vec()
                } else {
                    (*i as i64).to_le_bytes().to_vec()
                }
            }
            Value::Float(f) => f.to_le_bytes().to_vec(),
            Value::Bool(b) => vec![if *b { 1 } else { 0 }],
            Value::Str(s) => {
                let b = s.borrow();
                let mut out = (b.len() as u64).to_le_bytes().to_vec();
                out.extend_from_slice(&b);
                out
            }
            Value::Class(c) => {
                let d = c.borrow();
                self.class_to_bytes(&d.name, &d.fields).unwrap_or_default()
            }
            Value::Ptr(p) => self.value_to_bytes(&p.borrow()),
            Value::Boxed(b) => self.value_to_bytes(&b.borrow().data.borrow()),
            _ => vec![],
        }
    }

    /// 任意值 → JSON 字符串
    pub(crate) fn value_to_json(&self, v: &Value) -> String {
        match v {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => {
                let s = String::from_utf8_lossy(&s.borrow()).to_string();
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
            Value::Arr(a) => {
                let items: Vec<String> = a
                    .borrow()
                    .iter()
                    .map(|c| self.value_to_json(&c.borrow()))
                    .collect();
                format!("[{}]", items.join(","))
            }
            // 集合（G4）：Vec 序列化为数组
            Value::Vec(d) => {
                let items: Vec<String> = d
                    .borrow()
                    .items
                    .borrow()
                    .iter()
                    .map(|c| self.value_to_json(&c.borrow()))
                    .collect();
                format!("[{}]", items.join(","))
            }
            // 集合（G4）：Map 序列化为对象
            Value::Map(m) => {
                let items: Vec<String> = m
                    .borrow()
                    .fields
                    .iter()
                    .map(|(k, v)| format!("\"{k}\":{}", self.value_to_json(v)))
                    .collect();
                format!("{{{}}}", items.join(","))
            }
            Value::Class(c) => {
                let d = c.borrow();
                if d.name == "Map" {
                    let items: Vec<String> = d
                        .fields
                        .iter()
                        .map(|(k, v)| format!("\"{k}\":{}", self.value_to_json(v)))
                        .collect();
                    format!("{{{}}}", items.join(","))
                } else {
                    let items: Vec<String> = d
                        .fields
                        .iter()
                        .map(|(k, v)| format!("\"{k}\":{}", self.value_to_json(v)))
                        .collect();
                    format!("{{{}}}", items.join(","))
                }
            }
            Value::Opt(Some(v)) => self.value_to_json(v),
            Value::Opt(None) => "null".to_string(),
            Value::Ptr(p) => self.value_to_json(&p.borrow()),
            Value::Boxed(b) => self.value_to_json(&b.borrow().data.borrow()),
            _ => "null".to_string(),
        }
    }

    /// JSON 对象字符串 → (key, value) 表（递归解析：支持嵌套对象/数组/字符串转义）
    pub(crate) fn parse_json_obj(&self, s: &str) -> Result<HashMap<String, Value>> {
        let (v, _) = self.parse_json_value(s)?;
        match v {
            Value::Class(c) => Ok(c.borrow().fields.clone()),
            _ => Ok(HashMap::new()),
        }
    }

    /// JSON 值解析（递归下降；返回 (值, 消费字节数)）
    pub(crate) fn parse_json_value(&self, s: &str) -> Result<(Value, usize)> {
        let s = s.trim_start();
        match s.as_bytes().first().copied() {
            Some(b'{') => self.parse_json_object(s),
            Some(b'[') => self.parse_json_array(s),
            Some(b'"') => self.parse_json_string(s),
            Some(b't') if s.starts_with("true") => Ok((Value::Bool(true), 4)),
            Some(b'f') if s.starts_with("false") => Ok((Value::Bool(false), 5)),
            Some(b'n') if s.starts_with("null") => Ok((Value::Opt(None), 4)),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_json_number(s),
            _ => Err(RtError::msg("InvalidJson", "unexpected token")),
        }
    }

    pub(crate) fn parse_json_object(&self, s: &str) -> Result<(Value, usize)> {
        let b = s.as_bytes();
        let mut out = HashMap::new();
        let mut pos = 1usize; // 跳过 '{'
        loop {
            while pos < b.len() && b[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= b.len() {
                return Err(RtError::msg("InvalidJson", "unterminated object"));
            }
            if b[pos] == b'}' {
                return Ok((Value::class("Map", out), pos + 1));
            }
            if b[pos] != b'"' {
                return Err(RtError::msg("InvalidJson", "expected string key"));
            }
            let (key, klen) = self.parse_json_string(&s[pos..])?;
            pos += klen;
            while pos < b.len() && b[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= b.len() || b[pos] != b':' {
                return Err(RtError::msg("InvalidJson", "expected ':'"));
            }
            pos += 1;
            let (val, vlen) = self.parse_json_value(&s[pos..])?;
            pos += vlen;
            if let Value::Str(ks) = key {
                out.insert(String::from_utf8_lossy(&ks.borrow()).to_string(), val);
            }
            while pos < b.len() && b[pos].is_ascii_whitespace() {
                pos += 1;
            }
            match b.get(pos).copied() {
                Some(b',') => pos += 1,
                Some(b'}') => return Ok((Value::class("Map", out), pos + 1)),
                _ => return Err(RtError::msg("InvalidJson", "expected ',' or '}'")),
            }
        }
    }

    pub(crate) fn parse_json_array(&self, s: &str) -> Result<(Value, usize)> {
        let b = s.as_bytes();
        let mut items = Vec::new();
        let mut pos = 1usize; // 跳过 '['
        loop {
            while pos < b.len() && b[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= b.len() {
                return Err(RtError::msg("InvalidJson", "unterminated array"));
            }
            if b[pos] == b']' {
                return Ok((Value::arr(items), pos + 1));
            }
            let (val, vlen) = self.parse_json_value(&s[pos..])?;
            pos += vlen;
            items.push(val);
            while pos < b.len() && b[pos].is_ascii_whitespace() {
                pos += 1;
            }
            match b.get(pos).copied() {
                Some(b',') => pos += 1,
                Some(b']') => return Ok((Value::arr(items), pos + 1)),
                _ => return Err(RtError::msg("InvalidJson", "expected ',' or ']'")),
            }
        }
    }

    pub(crate) fn parse_json_string(&self, s: &str) -> Result<(Value, usize)> {
        let b = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 1usize; // 跳过开引号
        while i < b.len() {
            match b[i] {
                b'"' => return Ok((Value::str_bytes(out), i + 1)),
                b'\\' => {
                    i += 1;
                    if i >= b.len() {
                        return Err(RtError::msg("InvalidJson", "bad escape"));
                    }
                    match b[i] {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        _ => return Err(RtError::msg("InvalidJson", "unknown escape")),
                    }
                    i += 1;
                }
                c => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        Err(RtError::msg("InvalidJson", "unterminated string"))
    }

    pub(crate) fn parse_json_number(&self, s: &str) -> Result<(Value, usize)> {
        let b = s.as_bytes();
        let mut i = 0usize;
        while i < b.len()
            && (b[i].is_ascii_digit() || matches!(b[i], b'-' | b'+' | b'.' | b'e' | b'E'))
        {
            i += 1;
        }
        let text = &s[..i];
        let v = if text.contains('.') || text.contains('e') || text.contains('E') {
            text.parse::<f64>()
                .map(Value::Float)
                .map_err(|_| RtError::msg("InvalidJson", "bad number"))?
        } else {
            match text.parse::<i128>() {
                Ok(n) => Value::Int(n),
                Err(_) => text
                    .parse::<f64>()
                    .map(Value::Float)
                    .map_err(|_| RtError::msg("InvalidJson", "bad number"))?,
            }
        };
        Ok((v, i))
    }

    /// JSON 对象 → class 实例（匹配字段名；嵌套对象按字段声明类型还原）
    pub(crate) fn class_from_json(
        &mut self,
        ty: &str,
        obj: &HashMap<String, Value>,
    ) -> Result<Value> {
        let Some(TypeDef::Class { fields: fdecls, .. }) = self.types.get(ty) else {
            return Err(RtError::msg("UnknownType", format!("unknown type `{ty}`")));
        };
        // 先克隆字段声明：json_coerce/default_value（&mut self）具体化会重新借用 self
        let fdecls = fdecls.clone();
        let mut f = HashMap::new();
        for fd in &fdecls {
            let v = match obj.get(&fd.name) {
                Some(v) => self.json_coerce(&fd.ty, v),
                None => self.default_value(Some(&fd.ty)).unwrap_or(Value::Void),
            };
            f.insert(fd.name.clone(), v);
        }
        Ok(Value::class(ty, f))
    }

    /// 解析后的 JSON 值 → 字段声明类型（嵌套对象（Map）还原为目标 class；集合元素递归还原）
    pub(crate) fn json_coerce(&mut self, ty: &Type, v: &Value) -> Value {
        match ty.strip() {
            Type::Named(n, args) => {
                // 集合元素类型还原（Vec(T)/Deque(T) 中的嵌套对象）
                if (n == "Vec" || n == "Deque") && !args.is_empty() {
                    let items: Vec<Value> = match v {
                        Value::Arr(a) => a
                            .borrow()
                            .iter()
                            .map(|c| self.json_coerce(&args[0], &c.borrow()))
                            .collect(),
                        // G4：集合为 Vec 句柄（items 同 Arr 存储）
                        Value::Vec(d) => d
                            .borrow()
                            .items
                            .borrow()
                            .iter()
                            .map(|c| self.json_coerce(&args[0], &c.borrow()))
                            .collect(),
                        _ => return v.clone(),
                    };
                    return Value::arr(items);
                }
                match v {
                    Value::Class(c) => {
                        let d = c.borrow();
                        if d.name == "Map" && n != "Map" && self.types.contains_key(n) {
                            // 嵌套 heap class：通用 JSON 对象（Map）→ 目标 class
                            return self
                                .class_from_json(n, &d.fields.clone())
                                .unwrap_or_else(|_| v.clone());
                        }
                    }
                    // 集合（G4）：Value::Map 同样可还原为 heap class
                    Value::Map(m) => {
                        if n != "Map" && self.types.contains_key(n) {
                            return self
                                .class_from_json(n, &m.borrow().fields.clone())
                                .unwrap_or_else(|_| v.clone());
                        }
                    }
                    _ => {}
                }
                v.clone()
            }
            _ => v.clone(),
        }
    }

    pub(crate) fn deep_copy(&self, v: Value) -> Value {
        match v {
            Value::Arr(a) => {
                let items: Vec<Value> = a
                    .borrow()
                    .iter()
                    .map(|c| self.deep_copy(c.borrow().clone()))
                    .collect();
                Value::arr(items)
            }
            Value::Class(c) => {
                let d = c.borrow();
                let fields: HashMap<String, Value> = d
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.deep_copy(v.clone())))
                    .collect();
                Value::class(&d.name, fields)
            }
            Value::Str(s) => Value::Str(Rc::new(RefCell::new(s.borrow().clone()))),
            Value::Ptr(p) => Value::Ptr(Rc::new(RefCell::new(self.deep_copy(p.borrow().clone())))),
            // 装箱胖指针：data 深拷贝（新 cell），vtbl/alloc 原样携带
            Value::Boxed(b) => {
                let d = b.borrow();
                // data 先克隆为持有值（链式 Ref 借用不可跨块尾表达式）
                let data = Rc::new(RefCell::new(self.deep_copy(d.data.borrow().clone())));
                Value::Boxed(Rc::new(RefCell::new(BoxedData {
                    data,
                    vtbl: d.vtbl.clone(),
                    alloc: d.alloc.clone(),
                })))
            }
            // 集合（G4）：items/fields 深拷贝，alloc 原样携带
            Value::Vec(v) => {
                let d = v.borrow();
                let items: Vec<Value> = d
                    .items
                    .borrow()
                    .iter()
                    .map(|c| self.deep_copy(c.borrow().clone()))
                    .collect();
                Value::vec(items, d.alloc.clone())
            }
            Value::Map(m) => {
                let d = m.borrow();
                let fields: HashMap<String, Value> = d
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.deep_copy(v.clone())))
                    .collect();
                Value::map(fields, d.alloc.clone())
            }
            Value::Opt(Some(v)) => Value::Opt(Some(Rc::new(self.deep_copy((*v).clone())))),
            // move 捕获闭包值：环境逐 cell 深拷贝——闭包持有独立环境副本
            // （与原作用域/其他闭包脱离共享；`Rc<RefCell>` 非 Copy 语义补齐）。
            Value::Closure(c) => Value::Closure(ClosureData {
                params: c.params.clone(),
                body: c.body.clone(),
                is_mut: c.is_mut,
                is_move: c.is_move,
                env: c
                    .env
                    .iter()
                    .map(|m| {
                        m.iter()
                            .map(|(k, cell)| {
                                let v = self.deep_copy(cell.borrow().clone());
                                (k.clone(), Rc::new(RefCell::new(v)))
                            })
                            .collect()
                    })
                    .collect(),
            }),
            other => other,
        }
    }

    /// 浅复制（CopyMode.shallow，L1）：顶层容器新建，元素共享槽（内存问题用户负责）
    pub(crate) fn shallow_copy(&self, v: Value) -> Value {
        match v {
            Value::Arr(a) => {
                let items = a.borrow().clone();
                Value::Arr(Rc::new(RefCell::new(items)))
            }
            Value::Class(c) => {
                let d = c.borrow();
                Value::class(&d.name, d.fields.clone())
            }
            Value::Ptr(p) => Value::Ptr(p),
            // 装箱胖指针浅复制：共享 data cell（内存问题用户负责，同 Ptr）
            Value::Boxed(b) => Value::Boxed(b),
            // 集合（G4）浅复制：新容器共享 items/fields，alloc 原样携带
            Value::Vec(v) => Value::Vec(Rc::new(RefCell::new(VecData {
                items: v.borrow().items.clone(),
                alloc: v.borrow().alloc.clone(),
            }))),
            Value::Map(m) => Value::Map(Rc::new(RefCell::new(MapData {
                fields: m.borrow().fields.clone(),
                alloc: m.borrow().alloc.clone(),
            }))),
            other => other,
        }
    }

    // ---------- 入口 ----------

    /// 运行 main；错误返回 RtError（入口错误由运行时统一报告，06-language-spec）
    pub fn run_main(&mut self) -> Result<()> {
        self.in_main = true;
        let io = self.io_value();
        self.bind("io", io.clone());
        self.bind("alloc", Value::Alloc);
        let has_main = self.funcs.contains_key("main");
        if !has_main {
            self.in_main = false;
            return Err(RtError::msg("NoMain", "no `main` entry point"));
        }
        // main(args: owned Vec(String))——单参数 = 命令行参数（0 号 = 程序名）；或零参版本。
        // 2026-08-17 定案（ADR-0010）：main 不再注入 io（io 经 `import H.std.{io}` 引入）。
        let args_val = Value::vec(
            self.args.iter().map(|a| Value::str(a)).collect(),
            Value::Alloc,
        );
        // pick_fn 候选唯一时短路返回、不校验参数个数；故按参数个数精确选：
        // 1 参 → 传 args；否则传 []。
        let main_def = if self
            .funcs
            .get("main")
            .map_or(false, |fns| fns.iter().any(|f| f.params.len() == 1))
        {
            self.pick_fn("main", &[args_val.clone()])?
        } else {
            self.pick_fn("main", &[])?
        };
        let main_args = if main_def.params.len() == 1 {
            vec![args_val]
        } else {
            vec![]
        };
        let r = self.call_fn(&main_def, &main_args, &Span::new(0, 0, 0, 0));
        self.in_main = false;
        // E2.2 根回收：main 返回后运行未 join/未 detach 的线程到完成（副作用发生）
        self.drain_root_threads();
        match r {
            // 未处理错误到达根作用域（值通道）：记录错误名位置后 panic 式中止
            Ok(Value::Err { name, code }) => {
                let e = RtError::new(&name, self.error_locs.get(&name).cloned());
                let e = e.with_code(code);
                Err(e)
            }
            Ok(_) => Ok(()),
            // io.exit：正常退出信号（exit_code 已记录）
            Err(e) if e.name == "ExitRequested" => Ok(()),
            Err(e) => {
                // M2.6：未处理错误到达根作用域 → 记录错误名位置（原始错误定位），
                // panic 式中止（无恢复/不输出调用链；hc-tools 打印后非零退出）
                if e.span.is_none() && !e.is_signal() {
                    if let Some(sp) = self.error_locs.get(&e.name).cloned() {
                        let mut e2 = e.clone();
                        e2.span = Some(sp);
                        return Err(e2);
                    }
                }
                Err(e)
            }
        }
    }

    /// 运行全部测试；返回 (passed, failed, skipped)
    pub fn run_tests(&mut self) -> (usize, usize, usize) {
        let mut tests: Vec<FnDef> = Vec::new();
        for fns in self.funcs.values() {
            for f in fns {
                if f.is_test {
                    tests.push(f.clone());
                }
            }
        }
        // 声明序
        tests.sort_by(|a, b| a.span.line.cmp(&b.span.line));
        let (mut passed, mut failed, mut skipped) = (0, 0, 0);
        for t in tests {
            if t.test_mode == TestMode::Thread {
                // D1-4：线程模式测试——在独立 OS 线程中执行，支持硬超时
                let (p, f, s, out) = self.run_test_threaded(&t);
                passed += p;
                failed += f;
                skipped += s;
                self.test_out.extend(out);
                continue;
            }
            // D1-5：每测试独立输出缓冲——保存主缓冲，创建临时缓冲
            let main_out = std::mem::take(&mut self.test_out);
            let start = std::time::Instant::now();
            self.push_scope();
            self.fail_info = None;
            let r = if t.test_mode == TestMode::Async {
                // D1-3：异步测试——使用 evented IO 模式，通过 Future 执行
                self.bind("io", self.io_value_with_runtime("evented"));
                self.bind("alloc", Value::Alloc);
                let fut = self.make_future(&t, vec![]);
                self.future_run(&fut, &Span::new(0, 0, 0, 0))
            } else {
                // A3/ADR-0010：测试环境绑定 io（import H.std.{io} 等价；`&io` 传参走作用域 cell）
                self.bind("io", self.io_value());
                self.bind("alloc", Value::Alloc);
                self.exec_fn_body(&t.body, &[])
            };
            let _ = self.pop_scope(Self::is_err_path(&r.clone().map(Flow::Value)));
            // 显示名：名称 ?? 函数名
            let display = t.test_name.clone().unwrap_or_else(|| t.name.clone());
            // D1-2：测试超时检测——测试完成后检查耗时是否超过 timeout（秒）
            let timeout_exceeded = t.test_timeout.map_or(false, |timeout| {
                let elapsed = start.elapsed();
                elapsed.as_secs() >= timeout
            });
            match r {
                Ok(Value::Err { name, .. }) if name == "SkipTest" => {
                    // Q-T3：`return error.SkipTest` 到达测试根 → 统计为 SKIP
                    if timeout_exceeded {
                        self.test_out.push(format!("[FAIL] {} (timeout)", display));
                        failed += 1;
                    } else {
                        self.test_out.push(format!("[SKIP] {}", display));
                        skipped += 1;
                    }
                }
                Ok(Value::Err { name, .. }) => {
                    // M2.6：未处理错误到达测试根（值通道）→ 记 FAIL
                    if timeout_exceeded {
                        self.test_out.push(format!("[FAIL] {} (timeout)", display));
                    } else {
                        self.test_out
                            .push(format!("[FAIL] {} (error.{})", display, name));
                    }
                    failed += 1;
                }
                Ok(_) => {
                    if timeout_exceeded {
                        self.test_out.push(format!("[FAIL] {} (timeout)", display));
                        failed += 1;
                    } else {
                        self.test_out.push(format!("[PASS] {}", display));
                        passed += 1;
                    }
                }
                Err(e) if e.name == "SkipTest" => {
                    if timeout_exceeded {
                        self.test_out.push(format!("[FAIL] {} (timeout)", display));
                        failed += 1;
                    } else {
                        self.test_out.push(format!("[SKIP] {}", display));
                        skipped += 1;
                    }
                }
                Err(e) => {
                    let extra = self.fail_info.clone().unwrap_or_default();
                    if timeout_exceeded {
                        self.test_out.push(format!("[FAIL] {} (timeout)", display));
                    } else {
                        self.test_out.push(format!(
                            "[FAIL] {} (error.{}{})",
                            display,
                            e.name,
                            if extra.is_empty() {
                                "".into()
                            } else {
                                format!(": {extra}")
                            }
                        ));
                    }
                    failed += 1;
                }
            }
            // D1-5：测试完成后，将临时缓冲内容追加到主缓冲
            let test_output = std::mem::replace(&mut self.test_out, main_out);
            self.test_out.extend(test_output);
        }
        // E2.2 根回收：未 join/未 detach 的线程在全部测试结束后运行到完成
        self.drain_root_threads();
        (passed, failed, skipped)
    }

    /// D1-4：线程模式测试执行器——在独立 OS 线程中运行测试，支持硬超时。
    /// 克隆程序快照 + 环境数据到新 Interp，在子线程中重新 load + 执行，
    /// 通过 mpsc channel 返回结果；超时后标记为 FAIL 并继续（OS 线程无法强制终止）。
    fn run_test_threaded(&self, t: &FnDef) -> (usize, usize, usize, Vec<String>) {
        let program = self
            .program
            .clone()
            .expect("run_test_threaded: no program stored (load() not called?)");
        let source = self.source.clone();
        let args = self.args.clone();
        let extern_programs = self.extern_programs.clone();
        let dep_programs = self.dep_programs.clone();
        let import_env = self.import_env.clone();
        let t = t.clone();
        // 默认超时 5 秒；`[test(timeout=N)]` 覆盖
        let timeout_secs = t.test_timeout.unwrap_or(5);
        let display = t.test_name.clone().unwrap_or_else(|| t.name.clone());
        let display_clone = display.clone();

        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let mut child = Interp::new(&source);
            child.args = args;
            child.extern_programs = extern_programs;
            child.dep_programs = dep_programs;
            child.import_env = import_env;
            if let Err(e) = child.load(&program) {
                let _ = tx.send((
                    0,
                    1,
                    0,
                    vec![format!(
                        "[FAIL] {} (load error: {})",
                        display_clone, e.message
                    )],
                ));
                return;
            }
            let mut test_out = Vec::new();
            child.push_scope();
            child.fail_info = None;
            child.bind("io", child.io_value());
            child.bind("alloc", Value::Alloc);
            let r = child.exec_fn_body(&t.body, &[]);
            let _ = child.pop_scope(false);
            match r {
                Ok(Value::Err { name, .. }) if name == "SkipTest" => {
                    test_out.push(format!("[SKIP] {}", display_clone));
                    let _ = tx.send((0, 0, 1, test_out));
                }
                Ok(Value::Err { name, .. }) => {
                    test_out.push(format!("[FAIL] {} (error.{})", display_clone, name));
                    let _ = tx.send((0, 1, 0, test_out));
                }
                Ok(_) => {
                    test_out.push(format!("[PASS] {}", display_clone));
                    let _ = tx.send((1, 0, 0, test_out));
                }
                Err(e) => {
                    test_out.push(format!("[FAIL] {} (error.{})", display_clone, e.name));
                    let _ = tx.send((0, 1, 0, test_out));
                }
            }
        });

        match rx.recv_timeout(std::time::Duration::from_secs(timeout_secs)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                (0, 1, 0, vec![format!("[FAIL] {} (timeout)", display)])
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // 线程 panic 或异常退出；标记为 FAIL
                (0, 1, 0, vec![format!("[FAIL] {} (thread panic)", display)])
            }
        }
    }

    pub fn collect_tests(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for fns in self.funcs.values() {
            for f in fns {
                if f.is_test {
                    names.push(f.name.clone());
                }
            }
        }
        names.sort();
        names
    }

    /// 读取全局变量当前值（测试观察用，尤其 run_tests 之后检查根回收线程的副作用）
    pub fn global_value(&self, name: &str) -> Option<Value> {
        self.globals.get(name).map(|cell| cell.borrow().clone())
    }
}
