//! Quality checks for Roxy codebase
//!
//! Run with: cargo xtask check

use anyhow::{Context, Result};
use colored::*;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("check") => run_quality_checks(),
        Some("fmt") => run_fmt(false),
        Some("fmt-check") => run_fmt(true),
        Some("clippy") => run_clippy(),
        Some("test") => run_tests(),
        _ => {
            println!("Usage: cargo xtask <command>");
            println!();
            println!("Commands:");
            println!("  check       Run all quality checks");
            println!("  fmt         Format code with rustfmt");
            println!("  fmt-check   Check code formatting");
            println!("  clippy      Run clippy lints");
            println!("  test        Run all tests");
            Ok(())
        }
    }
}

fn run_quality_checks() -> Result<()> {
    println!("{}", "🔍 Running Roxy code quality checks...".bold().blue());
    println!();

    let mut failures = 0;

    // Error Handling
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());
    println!("{}", "📦 Error Handling".bold());
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());

    if let Err(e) = check_unwraps() {
        println!("{} {}", "✗".red().bold(), e);
        failures += 1;
    }

    if let Err(e) = check_expects() {
        println!("{} {}", "✗".red().bold(), e);
        failures += 1;
    }

    // Security
    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());
    println!("{}", "🔐 Security".bold());
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());

    if let Err(e) = check_sql_injection() {
        println!("{} {}", "✗".red().bold(), e);
        failures += 1;
    }

    if let Err(e) = check_credential_leaks() {
        println!("{} {}", "✗".red().bold(), e);
        failures += 1;
    }

    // Code Maintenance
    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());
    println!("{}", "📝 Code Maintenance".bold());
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());

    if let Err(e) = check_todos() {
        println!("{} {}", "⚠".yellow().bold(), e);
    }

    // Formatting
    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());
    println!("{}", "🎨 Code Formatting".bold());
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());

    if let Err(e) = run_fmt(true) {
        println!("{} {}", "✗".red().bold(), e);
        failures += 1;
    }

    // Linting
    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());
    println!("{}", "🔍 Linting".bold());
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());

    if let Err(e) = run_clippy() {
        println!("{} {}", "✗".red().bold(), e);
        failures += 1;
    }

    // Testing
    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());
    println!("{}", "🧪 Testing".bold());
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());

    if let Err(e) = check_test_coverage() {
        println!("{} {}", "⚠".yellow().bold(), e);
    }

    // Dependencies
    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());
    println!("{}", "📦 Dependencies".bold());
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());

    if let Err(e) = check_dependencies() {
        println!("{} {}", "⚠".yellow().bold(), e);
    }

    // Summary
    println!();
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());
    println!("{}", "📊 Summary".bold());
    println!("{}", "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".bold());

    if failures == 0 {
        println!("{} All checks passed!", "✓".green().bold());
        Ok(())
    } else {
        println!("{} {} check(s) failed", "✗".red().bold(), failures);
        println!();
        println!("Review the failures above and fix them before committing.");
        std::process::exit(1);
    }
}

fn check_unwraps() -> Result<()> {
    let unwraps = count_pattern(r"\.unwrap\(\)", &["crates"], &["tests", "target"])?;

    print!("Checking for .unwrap() in production code... ");
    if unwraps > 20 {
        Err(anyhow::anyhow!(
            "Found {} occurrences (target: ≤20)",
            unwraps
        ))
    } else {
        println!("{} Found {} occurrences", "✓".green().bold(), unwraps);
        Ok(())
    }
}

fn check_expects() -> Result<()> {
    let expects = count_pattern(r"\.expect\(", &["crates"], &["tests", "target"])?;

    print!("Checking for .expect() in production code... ");
    if expects > 15 {
        Err(anyhow::anyhow!(
            "Found {} occurrences (target: ≤15)",
            expects
        ))
    } else {
        println!("{} Found {} occurrences", "✓".green().bold(), expects);
        Ok(())
    }
}

fn check_sql_injection() -> Result<()> {
    print!("Checking for SQL injection patterns... ");

    let sql_format = count_pattern(
        r#"format!\([^)]*"[^"]*(?:SELECT|INSERT|UPDATE|DELETE)"#,
        &["crates"],
        &["tests", "target"],
    )?;

    if sql_format > 0 {
        return Err(anyhow::anyhow!(
            "Found {} format! calls with SQL queries (use .bind() instead)",
            sql_format
        ));
    }

    let escape_string = count_pattern(r"escape_string", &["crates"], &["tests", "target"])?;

    if escape_string > 0 {
        return Err(anyhow::anyhow!(
            "Found {} escape_string calls (use .bind() instead)",
            escape_string
        ));
    }

    println!("{}", "✓".green().bold());
    Ok(())
}

fn check_credential_leaks() -> Result<()> {
    print!("Checking for potential credential leaks... ");

    // Filter out cases with "redacted" or "sanitized"
    let dangerous_leaks = find_pattern_with_filter(
        r#"tracing::[^(]*\([^)]*(?:token|password|secret|key)"#,
        &["crates"],
        &["tests", "target"],
        |line| !line.contains("redacted") && !line.contains("sanitized"),
    )?;

    if !dangerous_leaks.is_empty() {
        println!(
            "{} Found {} potential credential leaks:",
            "✗".red().bold(),
            dangerous_leaks.len()
        );
        for (file, line_num, line) in dangerous_leaks.iter().take(3) {
            println!("  {}:{}: {}", file.display(), line_num, line.trim());
        }
        return Err(anyhow::anyhow!(""));
    }

    println!("{}", "✓".green().bold());
    Ok(())
}

fn check_todos() -> Result<()> {
    let todos = count_pattern(
        r"(?:TODO|FIXME|XXX|HACK)",
        &["crates"],
        &["tests", "target"],
    )?;

    print!("Checking for TODO comments... ");
    if todos > 5 {
        Err(anyhow::anyhow!(
            "Found {} TODO comments (create GitHub issues)",
            todos
        ))
    } else {
        println!("{} Found {} TODO comments", "✓".green().bold(), todos);
        Ok(())
    }
}

fn check_test_coverage() -> Result<()> {
    let total_files = count_rust_files(&["crates"], &["tests", "target"])?;
    let files_with_tests = count_files_with_pattern(
        r"#\[test\]|#\[cfg\(test\)\]",
        &["crates"],
        &["tests", "target"],
    )?;

    let coverage_pct = if total_files > 0 {
        (files_with_tests * 100) / total_files
    } else {
        0
    };

    print!("Checking test coverage... ");
    if coverage_pct < 60 {
        Err(anyhow::anyhow!(
            "{}% of files have tests (target: ≥60%)",
            coverage_pct
        ))
    } else {
        println!(
            "{} {}% of files have tests",
            "✓".green().bold(),
            coverage_pct
        );
        Ok(())
    }
}

fn check_dependencies() -> Result<()> {
    print!("Checking for security vulnerabilities... ");

    // Check if cargo-audit is installed
    let audit_check = Command::new("cargo")
        .arg("audit")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match audit_check {
        Ok(status) if status.success() => {
            let output = Command::new("cargo")
                .arg("audit")
                .output()
                .context("Failed to run cargo audit")?;

            if output.status.success() {
                println!("{}", "✓".green().bold());
                Ok(())
            } else {
                Err(anyhow::anyhow!("cargo audit found vulnerabilities"))
            }
        }
        _ => {
            println!(
                "{} cargo-audit not installed (run: cargo install cargo-audit)",
                "⚠".yellow().bold()
            );
            Ok(())
        }
    }
}

fn run_fmt(check_only: bool) -> Result<()> {
    print!("Checking formatting... ");

    let mut cmd = Command::new("cargo");
    cmd.arg("fmt");

    if check_only {
        cmd.arg("--check");
    }

    let status = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to run cargo fmt")?;

    if status.success() {
        println!("{}", "✓".green().bold());
        Ok(())
    } else {
        if check_only {
            Err(anyhow::anyhow!(
                "Code is not formatted (run: cargo xtask fmt)"
            ))
        } else {
            Err(anyhow::anyhow!("Failed to format code"))
        }
    }
}

fn run_clippy() -> Result<()> {
    print!("Running clippy lints... ");

    let output = Command::new("cargo")
        .args([
            "clippy",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ])
        .output()
        .context("Failed to run cargo clippy")?;

    if output.status.success() {
        println!("{}", "✓".green().bold());
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Print first few lines of clippy output
        for line in stderr.lines().take(10) {
            println!("  {}", line);
        }
        Err(anyhow::anyhow!("Clippy found issues"))
    }
}

fn run_tests() -> Result<()> {
    println!("{}", "Running tests...".bold().blue());

    let status = Command::new("cargo")
        .args(["test", "--all"])
        .status()
        .context("Failed to run tests")?;

    if status.success() {
        println!();
        println!("{} All tests passed!", "✓".green().bold());
        Ok(())
    } else {
        Err(anyhow::anyhow!("Tests failed"))
    }
}

// Helper functions

fn count_pattern(pattern: &str, include_dirs: &[&str], exclude_dirs: &[&str]) -> Result<usize> {
    let re = Regex::new(pattern)?;
    let mut count = 0;

    for dir in include_dirs {
        for entry in WalkDir::new(dir)
            .into_iter()
            .filter_entry(|e| !should_exclude(e.path(), exclude_dirs))
        {
            let entry = entry?;
            if entry.file_type().is_file() && entry.path().extension().map_or(false, |e| e == "rs")
            {
                let contents = std::fs::read_to_string(entry.path())?;
                count += re.find_iter(&contents).count();
            }
        }
    }

    Ok(count)
}

fn find_pattern_with_filter<F>(
    pattern: &str,
    include_dirs: &[&str],
    exclude_dirs: &[&str],
    filter: F,
) -> Result<Vec<(PathBuf, usize, String)>>
where
    F: Fn(&str) -> bool,
{
    let re = Regex::new(pattern)?;
    let mut matches = Vec::new();

    for dir in include_dirs {
        for entry in WalkDir::new(dir)
            .into_iter()
            .filter_entry(|e| !should_exclude(e.path(), exclude_dirs))
        {
            let entry = entry?;
            if entry.file_type().is_file() && entry.path().extension().map_or(false, |e| e == "rs")
            {
                let contents = std::fs::read_to_string(entry.path())?;
                for (line_num, line) in contents.lines().enumerate() {
                    if re.is_match(line) && filter(line) {
                        matches.push((entry.path().to_path_buf(), line_num + 1, line.to_string()));
                    }
                }
            }
        }
    }

    Ok(matches)
}

fn count_rust_files(include_dirs: &[&str], exclude_dirs: &[&str]) -> Result<usize> {
    let mut count = 0;

    for dir in include_dirs {
        for entry in WalkDir::new(dir)
            .into_iter()
            .filter_entry(|e| !should_exclude(e.path(), exclude_dirs))
        {
            let entry = entry?;
            if entry.file_type().is_file() && entry.path().extension().map_or(false, |e| e == "rs")
            {
                count += 1;
            }
        }
    }

    Ok(count)
}

fn count_files_with_pattern(
    pattern: &str,
    include_dirs: &[&str],
    exclude_dirs: &[&str],
) -> Result<usize> {
    let re = Regex::new(pattern)?;
    let mut count = 0;

    for dir in include_dirs {
        for entry in WalkDir::new(dir)
            .into_iter()
            .filter_entry(|e| !should_exclude(e.path(), exclude_dirs))
        {
            let entry = entry?;
            if entry.file_type().is_file() && entry.path().extension().map_or(false, |e| e == "rs")
            {
                let contents = std::fs::read_to_string(entry.path())?;
                if re.is_match(&contents) {
                    count += 1;
                }
            }
        }
    }

    Ok(count)
}

fn should_exclude(path: &Path, exclude_dirs: &[&str]) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .map_or(false, |s| exclude_dirs.contains(&s))
    })
}
