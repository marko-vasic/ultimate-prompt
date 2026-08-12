use std::process::Command;

fn main() {
    // Embed git revision hash if available
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        if output.status.success() {
            let rev = String::from_utf8_lossy(&output.stdout);
            println!("cargo:rustc-env=RIPGREP_BUILD_GIT_HASH={}", rev.trim());
        }
    }

    // Set a build timestamp
    println!(
        "cargo:rustc-env=RIPGREP_BUILD_DATE={}",
        chrono_lite_date()
    );
}

fn chrono_lite_date() -> String {
    // Simple date without pulling in chrono
    let output = Command::new("date")
        .args(["+%Y-%m-%d"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => "unknown".to_string(),
    }
}
