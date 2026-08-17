//! C1 CLI 测试：`hc run <目录>` 包加载形态——入口 `main.hc` 优先/首个 `.hc`、
//! 兄弟文件合并 + build.zon 依赖装载（复用 run_file 路径，无需 zig）。
//! F2：`io.exit`/`ExitType` 端到端——静默/打印/退出码（interpret 与 --ir 路径）。
//!
//! 用 `hc` 二进制（CARGO_BIN_EXE_hc-tools）驱动 CLI，断言输出与退出码。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static EXIT_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// F2：临时 .hc 文件（独立子目录，避免临时目录中其它 .hc 触发兄弟文件扫描噪音）
fn temp_hc_file(tag: &str, src: &str) -> PathBuf {
    let n = EXIT_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_exit_{}_{}_{}",
        std::process::id(),
        tag,
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.hc");
    std::fs::write(&path, src).unwrap();
    path
}

fn hc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hc"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples")
}

fn zig_cc_available() -> bool {
    Command::new("zig")
        .arg("cc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 递归复制目录（保留 app → ../jsonlib 的相对依赖结构）。
fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create dst dir");
    for entry in std::fs::read_dir(src).expect("read src dir") {
        let entry = entry.expect("read entry");
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir_all(&entry.path(), &to);
        } else {
            // 跳过既有构建产物（*.exe/*.a/*.sym/*.dll/*.lib/*.pdb），测试从源码态重新构建
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".exe")
                || name.ends_with(".a")
                || name.ends_with(".sym")
                || name.ends_with(".dll")
                || name.ends_with(".lib")
                || name.ends_with(".pdb")
            {
                continue;
            }
            std::fs::copy(entry.path(), to).expect("copy file");
        }
    }
}

#[test]
fn run_package_directory_uses_main_entry_and_deps() {
    // 02-packages/app：main.hc 入口 + build.zon 本地依赖 jsonlib（`import jsonlib.{parse}`）
    let out = Command::new(hc_bin())
        .arg("run")
        .arg(examples_dir().join("02-packages/app"))
        .output()
        .expect("run hc");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "目录 run 应成功: {stdout}{stderr}");
    assert!(
        stdout.contains("jsonlib.parse = 42"),
        "应输出依赖包调用结果: {stdout}{stderr}"
    );
}

#[test]
fn run_directory_prefers_main_hc_else_first_hc() {
    // 临时包目录：只有 a.hc（含 main）→ 首个 .hc 作入口
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_dir_entry_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(31) % 100000
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.hc"),
        "import H.std.{io};\nfn main(args: o Vec(String)) !void { io.print(\"first-hc\\n\"); }\n",
    )
    .unwrap();
    let out = Command::new(hc_bin())
        .arg("run")
        .arg(&dir)
        .output()
        .expect("run hc");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "首个 .hc 入口应成功: {stdout}");
    assert!(stdout.contains("first-hc"), "应运行 a.hc: {stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_directory_without_hc_errors() {
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_empty_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(17) % 100000
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(hc_bin())
        .arg("run")
        .arg(&dir)
        .output()
        .expect("run hc");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!out.status.success(), "空目录应失败");
    assert!(
        stderr.contains("无 .hc 文件"),
        "应提示无 .hc 文件: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_lib_static_archive_and_link_exe() {
    // C3：`hc build` 库形态——jsonlib（Kind::lib）→ libjsonlib.a + .sym；
    // app（Kind::exe，本地依赖）链接库 → main.exe，运行输出依赖函数结果。
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = examples_dir().join("02-packages");
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_c3_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(13) % 100000
    ));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir_all(&src, &dir);

    // 1) 库产出（静态归档 + 符号表）
    let out = Command::new(hc_bin())
        .arg("build")
        .arg(dir.join("jsonlib"))
        .output()
        .expect("hc build lib");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "库构建应成功: {stdout}{stderr}");
    assert!(
        dir.join("jsonlib/libjsonlib.a").exists(),
        "应产出 libjsonlib.a: {stdout}{stderr}"
    );
    assert!(dir.join("jsonlib/jsonlib.sym").exists(), "应产出 .sym 符号表");

    // 2) exe 链接本地库 + 运行
    let out = Command::new(hc_bin())
        .arg("build")
        .arg(dir.join("app"))
        .output()
        .expect("hc build app");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "app 构建应成功: {stdout}{stderr}");
    let exe = dir.join("app/main.exe");
    assert!(exe.exists(), "应产出 main.exe: {stdout}{stderr}");
    let run = Command::new(&exe).output().expect("run exe");
    let out_text = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(
        out_text.contains("jsonlib.parse = 42"),
        "exe 应调用依赖库函数输出 42: {out_text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_lib_dll_and_runtime_load() {
    // C4：`hc build --dll`——jsonlib（Kind::lib）→ jsonlib.dll（zig cc -shared）；
    // app（Kind::exe）依赖按 dll 构建并链接，dll 复制到 exe 目录供 OS 运行时加载。
    if !zig_cc_available() {
        eprintln!("SKIP: zig cc 不可用");
        return;
    }
    let src = examples_dir().join("02-packages");
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_c4_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(29) % 100000
    ));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir_all(&src, &dir);

    // 1) 库产 dll
    let out = Command::new(hc_bin())
        .arg("build")
        .arg("--dll")
        .arg(dir.join("jsonlib"))
        .output()
        .expect("hc build --dll lib");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "dll 库构建应成功: {stdout}{stderr}");
    assert!(
        dir.join("jsonlib/jsonlib.dll").exists(),
        "应产出 jsonlib.dll: {stdout}{stderr}"
    );

    // 2) exe 链接 dll + 运行时加载
    let out = Command::new(hc_bin())
        .arg("build")
        .arg("--dll")
        .arg(dir.join("app"))
        .output()
        .expect("hc build --dll app");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "app --dll 构建应成功: {stdout}{stderr}");
    let exe = dir.join("app/main.exe");
    assert!(exe.exists(), "应产出 main.exe: {stdout}{stderr}");
    assert!(
        dir.join("app/jsonlib.dll").exists(),
        "依赖 dll 应复制到 exe 目录供运行时加载: {stdout}{stderr}"
    );
    let run = Command::new(&exe).output().expect("run exe");
    let out_text = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(
        out_text.contains("jsonlib.parse = 42"),
        "exe 应经 dll 运行时加载调用依赖库输出 42: {out_text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_lib_with_main_is_diagnosed() {
    // C4：库无 main 校验——`Kind::lib` 含 main → 构建失败并诊断
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_libmain_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(37) % 100000
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("build.zon"),
        "const build = Build{ name = \"badlib\", version = \"0.1.0\", kind = Kind.lib, files = [\"lib.hc\"], deps = [] };\n",
    )
    .unwrap();
    std::fs::write(dir.join("lib.hc"), "fn main(args: o Vec(String)) !void { }\n").unwrap();
    let out = Command::new(hc_bin())
        .arg("build")
        .arg(&dir)
        .output()
        .expect("hc build lib-with-main");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!out.status.success(), "Kind::lib 含 main 应失败");
    assert!(
        stderr.contains("不应含 `main` 入口"),
        "应诊断库含 main: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_exit_success_is_silent_and_aborts() {
    // F2：ExitType.Exit——静默（无错误输出）、退出码 0、exit 后代码不执行
    let path = temp_hc_file(
        "ok",
        "import H.std.{io};\n\
         fn main(args: o Vec(String)) !void {\n\
             io.print(\"before\\n\");\n\
             io.exit(ExitType.Exit, 0);\n\
             io.print(\"after\\n\");\n\
         }\n",
    );
    let out = Command::new(hc_bin())
        .arg("run")
        .arg(&path)
        .output()
        .expect("run hc");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "exit 0 应成功: {stdout}{stderr}");
    assert!(stdout.contains("before"), "exit 前代码应执行: {stdout}");
    assert!(!stdout.contains("after"), "exit 后代码不应执行: {stdout}");
    assert!(!stderr.contains("error:"), "Exit 应静默无错误输出: {stderr}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn run_exit_nonzero_code_is_silent() {
    // F2：ExitType.Exit 非零码——静默、进程退出码 = 请求码
    let path = temp_hc_file(
        "code5",
        "import H.std.{io};\n\
         fn main(args: o Vec(String)) !void {\n\
             io.exit(ExitType.Exit, 5);\n\
         }\n",
    );
    let out = Command::new(hc_bin())
        .arg("run")
        .arg(&path)
        .output()
        .expect("run hc");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(out.status.code(), Some(5), "Exit 非零码应映射进程退出码: {stderr}");
    assert!(!stderr.contains("error:"), "Exit 应静默无错误输出: {stderr}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn run_exit_error_prints_and_codes() {
    // F2：ExitType.Error——打印错误消息、进程退出码 = 请求码
    let path = temp_hc_file(
        "err",
        "import H.std.{io};\n\
         fn main(args: o Vec(String)) !void {\n\
             io.exit(ExitType.Error, 3);\n\
         }\n",
    );
    let out = Command::new(hc_bin())
        .arg("run")
        .arg(&path)
        .output()
        .expect("run hc");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(out.status.code(), Some(3), "Error 非零码应映射进程退出码: {stderr}");
    assert!(
        stderr.contains("error: program exited with code 3"),
        "Error 应打印错误消息: {stderr}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn run_ir_exit_propagates_code() {
    // F2：`hc run --ir` 同语义——Error 打印 + 退出码传播（此前 IR 侧恒为 0）
    let path = temp_hc_file(
        "ir_err",
        "import H.std.{io};\n\
         fn main(args: o Vec(String)) !void {\n\
             io.exit(ExitType.Error, 3);\n\
         }\n",
    );
    let out = Command::new(hc_bin())
        .arg("run")
        .arg("--ir")
        .arg(&path)
        .output()
        .expect("run hc --ir");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(out.status.code(), Some(3), "IR 侧 Error 退出码应传播: {stderr}");
    assert!(
        stderr.contains("error: program exited with code 3"),
        "IR 侧 Error 应打印错误消息: {stderr}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn run_io_stdin_reads_line() {
    // F4：io.stdin() 从标准输入读一行（管道注入）——读取一行、去换行、原样回显
    let path = temp_hc_file(
        "stdin",
        "import H.std.{io};\n\
         fn main(args: o Vec(String)) !void {\n\
             var line = io.stdin();\n\
             io.print(\"got:\");\n\
             io.print(line);\n\
             io.print(\"\\n\");\n\
         }\n",
    );
    let mut child = Command::new(hc_bin())
        .arg("run")
        .arg(&path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn hc run");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hello-stdin\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait hc run");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "io.stdin 读取应成功: {stdout}{stderr}");
    assert!(
        stdout.contains("got:hello-stdin"),
        "应读到管道注入行（去换行）: {stdout}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn init_creates_runnable_scaffold() {
    // H1：`hc init <name>` → build.zon + main.hc → `hc run` / `hc test` 全绿
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_init_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(41) % 100000
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(hc_bin())
        .arg("init")
        .arg("demo")
        .current_dir(&dir)
        .output()
        .expect("run hc init");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "hc init 应成功: {stdout}{stderr}");
    let proj = dir.join("demo");
    assert!(proj.join("build.zon").exists(), "应生成 build.zon");
    assert!(proj.join("main.hc").exists(), "应生成 main.hc");
    let zon = std::fs::read_to_string(proj.join("build.zon")).expect("读 build.zon");
    assert!(zon.contains("name = \"demo\""), "清单应含项目名: {zon}");
    let main = std::fs::read_to_string(proj.join("main.hc")).expect("读 main.hc");
    assert!(main.contains("fn main(args: o Vec(String)) !void"), "应含标准入口: {main}");

    // 脚手架运行绿（目录 = 包，入口 main.hc）
    let run = Command::new(hc_bin())
        .arg("run")
        .arg(&proj)
        .output()
        .expect("run scaffold");
    let rs = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(run.status.success(), "脚手架 run 应成功: {rs}");
    assert!(rs.contains("hello, demo!"), "应输出问候语: {rs}");

    // 脚手架测试绿（[test] 冒烟测试）
    let test = Command::new(hc_bin())
        .arg("test")
        .arg(&proj)
        .output()
        .expect("test scaffold");
    let ts = String::from_utf8_lossy(&test.stdout).to_string();
    assert!(test.status.success(), "脚手架 test 应成功: {ts}");
    assert!(ts.contains("1 passed, 0 failed"), "应 1 passed 0 failed: {ts}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn init_refuses_nonempty_dir_and_bad_name() {
    // H1 安全：目录已存在且非空 → 拒绝覆盖（不触碰现有文件）
    let dir = std::env::temp_dir().join(format!(
        "hc_cli_init_bad_{}_{}",
        std::process::id(),
        std::process::id().wrapping_mul(53) % 100000
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("demo")).unwrap();
    std::fs::write(dir.join("demo/keep.txt"), "keep").unwrap();
    let out = Command::new(hc_bin())
        .arg("init")
        .arg("demo")
        .current_dir(&dir)
        .output()
        .expect("run hc init existing");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!out.status.success(), "非空目录应拒绝");
    assert!(stderr.contains("拒绝覆盖"), "应提示拒绝覆盖: {stderr}");
    assert!(
        std::fs::read_to_string(dir.join("demo/keep.txt")).ok().as_deref() == Some("keep"),
        "不应触碰现有文件"
    );

    // 非法名 → 用法错误退出码 2
    let out = Command::new(hc_bin())
        .arg("init")
        .arg("bad/name")
        .current_dir(&dir)
        .output()
        .expect("run hc init bad name");
    assert_eq!(out.status.code(), Some(2), "非法名应为用法错误");

    let _ = std::fs::remove_dir_all(&dir);
}
