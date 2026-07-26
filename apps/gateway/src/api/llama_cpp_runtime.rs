use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MIN_SUPPORTED_LLAMA_CPP_BUILD: u32 = 9592;

#[derive(Debug, Clone)]
pub struct LlamaCppServerStatus {
    pub path: PathBuf,
    pub build: Option<u32>,
    pub supported: bool,
    pub note: String,
}

pub fn current_binary_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

pub fn running_under_wsl() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    if std::env::var_os("WSL_INTEROP").is_some() || std::env::var_os("WSL_DISTRO_NAME").is_some() {
        return true;
    }
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|release| release.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

fn push_path_ancestors(path: Option<PathBuf>, out: &mut Vec<PathBuf>) {
    let Some(mut path) = path else {
        return;
    };
    loop {
        if out.iter().all(|existing| existing != &path) {
            out.push(path.clone());
        }
        if !path.pop() {
            break;
        }
    }
}

pub fn runtime_discovery_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    push_path_ancestors(current_binary_dir(), &mut roots);
    push_path_ancestors(std::env::current_dir().ok(), &mut roots);
    roots
}

pub fn first_existing_path<I>(candidates: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    candidates.into_iter().find(|path| path.exists())
}

fn push_llama_cpp_dir_candidates(dir: &Path, out: &mut Vec<PathBuf>) {
    out.push(dir.join("llama-server.exe"));
    out.push(dir.join("bin").join("llama-server.exe"));
    out.push(
        dir.join("build")
            .join("bin")
            .join("Release")
            .join("llama-server.exe"),
    );
}

fn discover_versioned_llama_cpp_candidates(root: &Path, out: &mut Vec<PathBuf>) {
    for base in [
        root.join("runtimes").join("llama.cpp"),
        root.join("llama.cpp"),
        root.join("external").join("llama.cpp"),
    ] {
        push_llama_cpp_dir_candidates(&base, out);
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if name.starts_with('b') && name[1..].chars().all(|ch| ch.is_ascii_digit()) {
                push_llama_cpp_dir_candidates(&path, out);
            }
        }
    }
}

pub fn discover_windows_llama_server_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(value) = std::env::var("BENSHU_WINDOWS_LLAMA_SERVER_EXE") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }

    for key in ["BENSHU_WINDOWS_LLAMA_CPP_DIR", "LLAMA_CPP_DIR"] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                push_llama_cpp_dir_candidates(Path::new(trimmed), &mut candidates);
            }
        }
    }

    for root in runtime_discovery_roots() {
        candidates.push(root.join("llama-server.exe"));
        candidates.push(root.join("bin").join("llama-server.exe"));
        discover_versioned_llama_cpp_candidates(&root, &mut candidates);
    }

    if running_under_wsl() {
        for drive in ["d", "c"] {
            let root = PathBuf::from(format!("/mnt/{drive}"));
            push_llama_cpp_dir_candidates(&root.join("llama.cpp"), &mut candidates);
            discover_versioned_llama_cpp_candidates(&root, &mut candidates);
        }
    }

    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|path| seen.insert(path.to_string_lossy().to_string()))
        .collect()
}

pub fn parse_llama_cpp_build(output: &str) -> Option<u32> {
    for line in output.lines() {
        let lowered = line.to_ascii_lowercase();
        if let Some(index) = lowered.find("version:") {
            let tail = &line[index + "version:".len()..];
            if let Some(build) = first_number(tail) {
                return Some(build);
            }
        }
        for marker in ["build: b", "build      : b", "build = b", "b"] {
            if let Some(index) = lowered.find(marker) {
                let tail = &line[index + marker.len()..];
                if let Some(build) = first_number(tail) {
                    return Some(build);
                }
            }
        }
    }
    None
}

fn first_number(value: &str) -> Option<u32> {
    let digits = value
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn infer_llama_cpp_build_from_path(path: &Path) -> Option<u32> {
    path.ancestors().find_map(|ancestor| {
        let name = ancestor.file_name()?.to_str()?;
        let lowered = name.to_ascii_lowercase();
        let digits = lowered.strip_prefix('b')?;
        if digits.chars().all(|ch| ch.is_ascii_digit()) {
            digits.parse().ok()
        } else {
            None
        }
    })
}

fn status_from_build_hint(path: &Path, build: u32, note: String) -> LlamaCppServerStatus {
    let supported = build >= MIN_SUPPORTED_LLAMA_CPP_BUILD;
    LlamaCppServerStatus {
        path: path.to_path_buf(),
        build: Some(build),
        supported,
        note,
    }
}

pub fn inspect_llama_cpp_server(path: &Path) -> LlamaCppServerStatus {
    if !path.exists() {
        return LlamaCppServerStatus {
            path: path.to_path_buf(),
            build: None,
            supported: false,
            note: "llama-server.exe was not found.".to_string(),
        };
    }

    let output = Command::new(path).arg("--version").output();
    let Ok(output) = output else {
        if let Some(build) = infer_llama_cpp_build_from_path(path) {
            return status_from_build_hint(
                path,
                build,
                format!(
                    "llama-server.exe version probe failed on this host, but path segment b{build} satisfies build inference."
                ),
            );
        }
        return LlamaCppServerStatus {
            path: path.to_path_buf(),
            build: None,
            supported: false,
            note: "llama-server.exe version probe failed.".to_string(),
        };
    };
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let build = parse_llama_cpp_build(&text).or_else(|| infer_llama_cpp_build_from_path(path));
    let supported = build.is_some_and(|value| value >= MIN_SUPPORTED_LLAMA_CPP_BUILD);
    let note = match build {
        Some(value) if supported => {
            format!("llama.cpp build b{value} satisfies minimum b{MIN_SUPPORTED_LLAMA_CPP_BUILD}.")
        }
        Some(value) => format!(
            "llama.cpp build b{value} is older than required b{MIN_SUPPORTED_LLAMA_CPP_BUILD}; update the bundled runtime before loading recent GGUF models."
        ),
        None => format!(
            "Could not parse llama.cpp build; required minimum is b{MIN_SUPPORTED_LLAMA_CPP_BUILD}."
        ),
    };
    LlamaCppServerStatus {
        path: path.to_path_buf(),
        build,
        supported,
        note,
    }
}

pub fn discover_windows_llama_server_status() -> Option<LlamaCppServerStatus> {
    let mut best_unsupported = None;
    for candidate in discover_windows_llama_server_candidates() {
        if !candidate.exists() {
            continue;
        }
        let status = inspect_llama_cpp_server(&candidate);
        if status.supported {
            return Some(status);
        }
        if best_unsupported.is_none() {
            best_unsupported = Some(status);
        }
    }
    best_unsupported
}

pub fn discover_supported_windows_llama_server() -> Option<LlamaCppServerStatus> {
    discover_windows_llama_server_status().filter(|status| status.supported)
}

pub fn llama_server_status_from_restart_command(
    command: &[String],
) -> Option<LlamaCppServerStatus> {
    if command.is_empty() {
        return None;
    }

    for arg in command {
        if let Some(path) = arg.strip_prefix("BENSHU_WINDOWS_LLAMA_SERVER_EXE=") {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return Some(inspect_llama_cpp_server(Path::new(trimmed)));
            }
        }
    }

    if let Some(path) = command_arg_after(command, "-ServerExe") {
        return Some(inspect_llama_cpp_server(Path::new(path)));
    }
    None
}

fn command_arg_after<'a>(command: &'a [String], flag: &str) -> Option<&'a str> {
    command
        .windows(2)
        .find(|pair| pair[0].eq_ignore_ascii_case(flag))
        .map(|pair| pair[1].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_llama_cpp_build_from_versioned_windows_path() {
        let path = Path::new("/mnt/d/llama.cpp/b9592/llama-server.exe");
        assert_eq!(infer_llama_cpp_build_from_path(path), Some(9592));
    }

    #[test]
    fn versioned_wsl_llama_cpp_candidates_include_direct_build_dirs() {
        let root =
            std::env::temp_dir().join(format!("benshu-llama-runtime-test-{}", std::process::id()));
        let versioned_dir = root.join("llama.cpp").join("b9592");
        std::fs::create_dir_all(&versioned_dir).expect("create versioned llama.cpp test dir");
        let mut candidates = Vec::new();
        discover_versioned_llama_cpp_candidates(&root, &mut candidates);
        let _ = std::fs::remove_dir_all(&root);
        assert!(candidates
            .iter()
            .any(|path| path.ends_with(Path::new("llama.cpp/b9592/llama-server.exe"))));
    }

    #[test]
    fn build_hint_status_marks_supported_builds() {
        let status = status_from_build_hint(
            Path::new("/mnt/d/llama.cpp/b9592/llama-server.exe"),
            9592,
            "test".to_string(),
        );
        assert_eq!(status.build, Some(9592));
        assert!(status.supported);
    }
}
