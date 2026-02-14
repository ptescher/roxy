use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Check that Docker is installed and the daemon is running.
pub fn check_docker() -> Result<()> {
    let output = Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("docker not found — is Docker installed and in PATH?")?;

    if !output.success() {
        bail!("Docker daemon is not running. Please start Docker Desktop or the Docker service.");
    }

    Ok(())
}

/// Return true if a Docker image with the given tag exists locally.
pub fn image_exists(tag: &str) -> Result<bool> {
    let output = Command::new("docker")
        .args(["image", "inspect", tag])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("failed to run docker image inspect")?;

    Ok(output.success())
}

/// Build a Docker image from the given context directory.
pub fn build_image(context_dir: &Path, dockerfile: &Path, tag: &str) -> Result<()> {
    tracing::info!("Building Docker image {tag}...");

    let status = Command::new("docker")
        .args([
            "build",
            "-t",
            tag,
            "-f",
            &dockerfile.to_string_lossy(),
            &context_dir.to_string_lossy(),
        ])
        .status()
        .context("failed to run docker build")?;

    if !status.success() {
        bail!("docker build failed (exit code {:?})", status.code());
    }

    tracing::info!("Image {tag} built successfully.");
    Ok(())
}

/// Execute `docker run` with the given arguments, inheriting stdio for
/// interactive use. Returns the exit status of the container.
pub fn run_container(args: &[String]) -> Result<std::process::ExitStatus> {
    let status = Command::new("docker")
        .arg("run")
        .args(args)
        .status()
        .context("failed to run docker container")?;

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_exists_nonexistent() {
        // A random tag that shouldn't exist
        let result = image_exists("roxy-sandbox-test-nonexistent-image:never");
        // If docker isn't available, skip silently
        if let Ok(exists) = result {
            assert!(!exists);
        }
    }
}
