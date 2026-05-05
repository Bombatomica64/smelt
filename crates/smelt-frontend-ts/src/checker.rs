use std::process::Command;

/// Runs the TypeScript and Oxlint checks for a generated file.
fn run_tsc(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("npx")
        .arg("tsc")
        .arg("--noEmit")
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(Box::new(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )));
    }
    Ok(())
}
/// Runs Oxlint on `path`.
fn run_oxlint(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("npx")
        .arg("oxlint")
        .arg("--type-aware")
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(Box::new(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )));
    }
    Ok(())
}

/// Checks a TypeScript source file with the local lint and type tools.
pub fn check(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    run_oxlint(path)?;
    run_tsc(path)
}
