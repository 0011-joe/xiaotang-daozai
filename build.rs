//! 把岛仔图标作为 RT_GROUP_ICON #1 编进 exe，供 LoadImageW(hInst, 1, ...) 取用。
//! 不引入额外依赖：直接调用 Windows SDK 的 rc.exe，把 .res 交给链接器。

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn find_rc() -> Option<PathBuf> {
    if let Ok(path) = env::var("RC") {
        let rc = PathBuf::from(path);
        if rc.is_file() {
            return Some(rc);
        }
    }
    // 项目内可放置 Windows SDK BuildTools 的 rc.exe（.tools/ 已被 gitignore）。
    if let Ok(root) = env::var("CARGO_MANIFEST_DIR") {
        let rc = PathBuf::from(root).join(".tools").join("rc").join("rc.exe");
        if rc.is_file() {
            return Some(rc);
        }
    }
    if let Ok(paths) = env::var("PATH") {
        for dir in env::split_paths(&paths) {
            let rc = dir.join("rc.exe");
            if rc.is_file() {
                return Some(rc);
            }
        }
    }
    let roots = [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ];
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let rc = dir.join("x64").join("rc.exe");
            if rc.exists() {
                found.push(rc);
            }
        }
    }
    found.sort();
    found.pop()
}

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/daozai.ico");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.tools/rc");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rustc-check-cfg=cfg(has_rc)");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("daozai.res");
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe 未找到，跳过 exe 图标/版本资源；托盘会使用内嵌 ICO 兜底");
        return;
    };
    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg("app.rc")
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        println!("cargo:warning=rc.exe 编译 app.rc 失败；托盘会使用内嵌 ICO 兜底");
        return;
    }
    println!("cargo:rustc-cfg=has_rc");
    println!("cargo:rustc-link-arg={}", out.display());
}
