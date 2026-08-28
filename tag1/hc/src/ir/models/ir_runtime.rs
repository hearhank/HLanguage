//! IR 运行时实例（ADR-0028：自 ir/mod.rs 拆分；Phase 5：共享堆 + 全局 cell + `@__init__` 一次性初始化）

use super::*;

/// 运行时实例（Phase 5）：共享堆 + 全局 cell + `@__init__` 一次性初始化。
/// 多测试/多函数共用同一实例时，全局只初始化一次、跨调用可见（对齐 oracle
/// `Interp` 的 `globals: HashMap`）。一致性套件与 `hc run --ir`/字节码 VM 走此路径。
#[derive(Debug, Default)]
pub struct IrRuntime {
    pub ctx: Ctx,
    inited: bool,
}

impl IrRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// 启动初始化（幂等）：预分配全部全局 cell（声明序）→ 按 funcs 序执行所有
    /// `@__init__` 函数（多文件合并 = 各模块 init 依次运行，entry 在前）。
    pub fn init(&mut self, module: &IrModule) -> R<()> {
        if self.inited {
            return Ok(());
        }
        self.inited = true;
        // E4：存储模块引用供 spawn 新线程使用
        self.ctx.module = Some(Arc::new(module.clone()));
        // G5：rng 默认状态（对齐 oracle Interp::new——seed(0) 亦回退该常量）
        self.ctx.rng_state = 0x9e37_79b9_7f4a_7c15;
        // 预分配全部全局 cell（声明序）——即使无全局也继续（保险：`@__init__` 仍须执行）。
        // Phase 7：隐式环境名（alloc/io/pi/Vec…）预置内建值（对齐 oracle 隐式环境注入）。
        for name in &module.globals {
            let v = if IMPLICIT_ENV.iter().any(|e| *e == name) {
                implicit_env_value(&mut self.ctx, name)
            } else {
                IrValue::Void
            };
            let cell = self.ctx.alloc(Cell::Value(v));
            self.ctx.globals.insert(name.clone(), cell);
        }
        for (idx, f) in module.funcs.iter().enumerate() {
            if f.name == "@__init__" {
                exec_func(&mut self.ctx, module, idx, &[], 0)?;
            }
        }
        Ok(())
    }

    /// 调用模块函数（自动先初始化全局）。
    pub fn call(&mut self, module: &IrModule, entry: &str, args: &[IrValue]) -> R<IrValue> {
        self.init(module)?;
        // main(args: owned Vec(String))——单参数 = 命令行参数（0 号 = 程序名）；或零参版本。
        // 2026-08-17 定案（ADR-0010）：main 不再注入 io（io 经 `import H.std.{io}` 引入）。
        let mut args = args.to_vec();
        if entry == "main" && args.is_empty() {
            let has_1p = module.func_index.get("main").map_or(false, |v| {
                v.iter().any(|&i| module.funcs[i].params.len() == 1)
            });
            if has_1p {
                let items = self
                    .ctx
                    .args
                    .iter()
                    .map(|a| IrValue::String(StringDataIr::from_slice(a.as_slice())))
                    .collect();
                let alloc = implicit_env_value(&mut self.ctx, "alloc");
                args.push(make_vec_with(&mut self.ctx, items, alloc));
            }
        }
        let idx = pick_func(&self.ctx, module, entry, &args)
            .ok_or_else(|| IrError::msg("NoFunction", format!("no function `{entry}`")))?;
        exec_func(&mut self.ctx, module, idx, &args, 0)
    }
}
