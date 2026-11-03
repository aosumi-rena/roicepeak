use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/branding/app-icon.ico");
    println!("cargo:rerun-if-changed=assets/branding/app-icon.rc");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let compiler = resource_compiler();
        let resource = output.join("app-icon.res");
        let status = Command::new(compiler)
            .current_dir(root.join("assets/branding"))
            .arg("/nologo")
            .arg("/fo")
            .arg(&resource)
            .arg("app-icon.rc")
            .status()
            .expect("Windows SDK resource compiler is required");
        assert!(
            status.success(),
            "Application icon resource compilation failed"
        );
        println!("cargo:rustc-link-arg-bin=roicepeak={}", resource.display());
    } else {
        let resource = output.join("app-icon.o");
        let status = Command::new("windres")
            .current_dir(root.join("assets/branding"))
            .args(["-i", "app-icon.rc", "-o"])
            .arg(&resource)
            .status()
            .expect("windres is required for Windows GNU builds");
        assert!(
            status.success(),
            "Application icon resource compilation failed"
        );
        println!("cargo:rustc-link-arg-bin=roicepeak={}", resource.display());
    }
}

fn resource_compiler() -> PathBuf {
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-env-changed=WindowsSdkVerBinPath");
    for directory in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
        let candidate = directory.join("rc.exe");
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(directory) = env::var_os("WindowsSdkVerBinPath") {
        let candidate = PathBuf::from(directory).join("x64/rc.exe");
        if candidate.is_file() {
            return candidate;
        }
    }
    let sdk = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("ProgramFiles(x86)").expect("Windows SDK is required"))
                .join("Windows Kits/10")
        });
    let mut versions: Vec<_> = fs::read_dir(sdk.join("bin"))
        .expect("Windows SDK bin directory is required")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("x64/rc.exe").is_file())
        .collect();
    versions.sort();
    versions
        .pop()
        .expect("Windows SDK rc.exe is required")
        .join("x64/rc.exe")
}
