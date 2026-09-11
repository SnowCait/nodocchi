use std::process::Command;

fn main() {
    println!("cargo::rustc-env=GIT_REVISION={}", git_revision());
    emit_rerun_if_head_changed();
}

fn git_revision() -> String {
    git_output(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string())
}

fn emit_rerun_if_head_changed() {
    let Some(head_path) = git_output(&["rev-parse", "--git-path", "HEAD"]) else {
        return;
    };
    println!("cargo::rerun-if-changed={head_path}");

    if let Some(branch_ref) = git_output(&["symbolic-ref", "--quiet", "HEAD"])
        && let Some(ref_path) = git_output(&["rev-parse", "--git-path", &branch_ref])
    {
        println!("cargo::rerun-if-changed={ref_path}");
    }
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}
