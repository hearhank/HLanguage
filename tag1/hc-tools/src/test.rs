//! `hc test`：收集并运行 `test fn` + Q-T5 编译模式交叉验证。

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

use hc_rt::Interp;

use crate::cli::{color_test_line, err_color, out_color, paint, DangleMode, TestMode};
use crate::fsio::{collect_hc_files, link_exe, zig_cc_available};
use crate::package::programs_to_test_ll;
use crate::run::{load_manifest_deps_into, report_leaks};
use crate::scriptgen;

/// Q-T5 编译模式交叉验证的临时产物目录序号（避免并行冲突）。
static TEST_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

type ParsedFile = (PathBuf, String, hc::Program);

/// Q-T5：编译模式交叉验证——原生 runner 退出码 vs 解释器该文件聚合结果。
/// 「解释器该文件有失败」⟺「原生退出码非 0」一致返回 Ok；不一致返回 Err（含诊断）。
/// 中间产物（.ll/.exe/.pdb）写到系统临时目录，运行后清理——不污染源码目录。
fn cross_validate_native(
    source: &str,
    entry: &hc::Program,
    siblings: &[&hc::Program],
    interp_fail: usize,
) -> Result<(), String> {
    let ll = programs_to_test_ll(entry, source, siblings)?;
    let n = TEST_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let work = std::env::temp_dir().join(format!("hc_test_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&work).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let ll_path = work.join("prog.ll");
    std::fs::write(&ll_path, &ll).map_err(|e| format!("写入 {} 失败: {e}", ll_path.display()))?;
    let exe_name = if cfg!(windows) { "prog.exe" } else { "prog" };
    let exe_path = work.join(exe_name);
    if let Err(e) = link_exe(&ll_path, &exe_path, &[]) {
        let _ = std::fs::remove_dir_all(&work);
        return Err(e);
    }
    let out = match std::process::Command::new(&exe_path).output() {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&work);
            return Err(format!("运行 {} 失败: {e}", exe_path.display()));
        }
    };
    let _ = std::fs::remove_dir_all(&work);

    let native_green = out.status.success();
    let interp_green = interp_fail == 0;
    if interp_green == native_green {
        return Ok(());
    }
    let mut detail = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.is_empty() {
        if !detail.is_empty() {
            detail.push('\n');
        }
        detail.push_str(&stderr);
    }
    Err(format!(
        "解释器 {} 失败（{}）vs 原生退出 {}（{}）{}",
        interp_fail,
        if interp_green { "绿" } else { "红" },
        out.status
            .code()
            .map_or_else(|| "异常".into(), |c| c.to_string()),
        if native_green { "绿" } else { "红" },
        if detail.is_empty() {
            String::new()
        } else {
            format!("\n{detail}")
        }
    ))
}

pub(crate) fn test_dir(target: &Path, mode: TestMode) -> ExitCode {
    test_dir_dangle(target, mode, DangleMode::Auto)
}

/// C2（ADR-0016）：`hc test [--dangle=on|off|auto]`——设置悬垂检查模式后运行测试。
pub(crate) fn test_dir_dangle(target: &Path, mode: TestMode, dangle: DangleMode) -> ExitCode {
    let mut files: Vec<PathBuf> = Vec::new();
    if target.is_dir() {
        collect_hc_files(target, &mut files);
        files.sort();
    } else if target.extension().map_or(false, |e| e == "hc") {
        files.push(target.to_path_buf());
    } else {
        eprintln!("error: `{}` 不是目录或 .hc 文件", target.display());
        return ExitCode::from(2);
    }
    if files.is_empty() {
        eprintln!("error: 未找到 .hc 文件于 {}", target.display());
        return ExitCode::from(2);
    }
    // Q-T5：编译模式需 zig cc（原生后端）；缺失不静默降级
    if mode == TestMode::Compile && !zig_cc_available() {
        eprintln!("error: --mode=compile 需 zig cc（原生后端）；未检测到 zig");
        return ExitCode::FAILURE;
    }

    // M1.4：按目录分组（同目录 = 同包；跨目录独立）
    let mut groups: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> =
        std::collections::BTreeMap::new();
    for f in &files {
        let dir = f.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        groups.entry(dir).or_default().push(f.clone());
    }

    // M4-1：如果目标目录有 version.hc，执行编译时版本号自增
    if target.is_dir() {
        crate::versiongen::bump_version(target);
    }

    let mut total_p = 0usize;
    let mut total_f = 0usize;
    let mut total_s = 0usize;
    let mut total_mismatch = 0usize;
    let mut all_ok = true;

    for group in groups.values() {
        // 组内一次性解析（失败的文件单独报告）
        let mut parsed: Vec<ParsedFile> = Vec::new();
        let mut bad: Vec<(PathBuf, String)> = Vec::new();
        for f in group {
            let name = f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            match std::fs::read_to_string(f) {
                Ok(src) => match scriptgen::parse_with_scripts(&src) {
                    Ok((expanded, mut p)) => {
                        // M1-1：文件级命名空间自动推断
                        let project_root = scriptgen::find_project_root(f);
                        let ns_name = scriptgen::compute_namespace_name(f, project_root.as_deref());
                        scriptgen::infer_namespace(&mut p, &ns_name);
                        parsed.push((f.clone(), expanded, p));
                    }
                    Err(msg) => {
                        bad.push((f.clone(), "parse/script error".into()));
                        eprintln!("{} {name}: {}", paint(err_color(), "31", "[FAIL]"), msg);
                    }
                },
                Err(e) => bad.push((f.clone(), format!("io: {e}"))),
            }
        }
        for (f, err) in &bad {
            let name = f.file_name().unwrap_or_default().to_string_lossy();
            eprintln!("{} {name} ({err})", paint(err_color(), "31", "[FAIL]"));
            total_f += 1;
            all_ok = false;
        }
        for (idx, (f, source, program)) in parsed.iter().enumerate() {
            let name = f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut interp = Interp::new(source);
            // 同包兄弟符号（跳过其 test/main）
            let siblings: Vec<&hc::Program> = parsed
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .map(|(_, (_, _, p))| p)
                .collect();
            if !siblings.is_empty() {
                if let Err(e) = interp.load_siblings(&siblings) {
                    eprintln!(
                        "{} {name} (sibling load: {} {})",
                        paint(err_color(), "31", "[FAIL]"),
                        e.name,
                        e.message
                    );
                    total_f += 1;
                    all_ok = false;
                    continue;
                }
            }
            // M7.2：build.zon 本地依赖（using pkg.xxx 跨包访问）
            if load_manifest_deps_into(&mut interp, f).is_err() {
                total_f += 1;
                all_ok = false;
                continue;
            }
            if let Err(e) = interp.load(program) {
                eprintln!(
                    "{} {name} (load error: {})",
                    paint(err_color(), "31", "[FAIL]"),
                    e.name
                );
                total_f += 1;
                all_ok = false;
                continue;
            }
            let (p, fail, s) = interp.run_tests();
            total_p += p;
            total_f += fail;
            total_s += s;
            if fail > 0 {
                all_ok = false;
            }
            // G5/§8.3 Debug 泄漏检测：每个文件测试结束后报告泄漏（不改变通过判定）
            report_leaks(&name, &interp.leak_report());
            let on = out_color();
            for line in &interp.test_out {
                println!("{name}::{}", color_test_line(line, on));
            }
            // Q-T5：编译模式——原生 runner 退出码 vs 解释器该文件聚合结果交叉验证
            if mode == TestMode::Compile {
                match cross_validate_native(source, program, &siblings, fail) {
                    Ok(()) => println!("{} {name}", paint(out_color(), "32", "[MATCH]")),
                    Err(msg) => {
                        eprintln!("{} {name}: {msg}", paint(err_color(), "31", "[MISMATCH]"));
                        total_mismatch += 1;
                        all_ok = false;
                    }
                }
            }
        }
    }

    let on = out_color();
    println!(
        "{} passed, {} failed, {} skipped",
        paint(on, "32", &total_p.to_string()),
        paint(on, "31", &total_f.to_string()),
        paint(on, "33", &total_s.to_string()),
    );
    if mode == TestMode::Compile && total_mismatch > 0 {
        println!("{}", paint(on, "33", &format!("{total_mismatch} mismatch")));
    }
    if all_ok && total_f == 0 && total_mismatch == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
