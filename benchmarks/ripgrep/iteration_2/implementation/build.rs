fn main() {
    // Embed git revision hash if available
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        if output.status.success() {
            let hash = String::from_utf8_lossy(&output.stdout);
            println!("cargo:rustc-env=RG_GIT_HASH={}", hash.trim());
        }
    }
}
