use std::process::Command;

fn run_tsc(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("npx")
        .arg("tsc")
        .arg("--noEmit")
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            String::from_utf8_lossy(&output.stderr),
        )));
    }
    Ok(())
}
fn run_oxlint(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("npx").arg("oxlint").arg(path).output()?;
    if !output.status.success() {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            String::from_utf8_lossy(&output.stderr),
        )));
    }
    Ok(())
}

pub fn check(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    run_oxlint(path)?;
    run_tsc(path)
}
