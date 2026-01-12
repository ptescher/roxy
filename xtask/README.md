# xtask - Roxy Quality Checks

This is a Rust-based quality checker for the Roxy codebase, following the [cargo-xtask](https://github.com/matklad/cargo-xtask) pattern.

## Why xtask instead of a Bash script?

- ✅ **Consistent with the project stack** - Everything is Rust
- ✅ **Cross-platform** - Works on macOS, Linux, Windows
- ✅ **Type-safe** - Compile-time guarantees
- ✅ **Fast** - Compiled, not interpreted
- ✅ **Easy to extend** - Just write more Rust code

## Usage

```bash
# Run all quality checks
cargo xtask check

# Format code
cargo xtask fmt

# Check formatting without changing files
cargo xtask fmt-check

# Run clippy lints
cargo xtask clippy

# Run tests
cargo xtask test
```

Or use the cargo aliases:

```bash
# Same as: cargo xtask check
cargo check-quality
```

## What Gets Checked

### 1. Error Handling ✅
- Counts `.unwrap()` calls in production code (target: ≤20)
- Counts `.expect()` calls in production code (target: ≤15)

### 2. Security 🔐
- Detects SQL injection patterns (`format!` with SQL)
- Detects manual `escape_string` usage
- Checks for credential leaks in logs

### 3. Code Maintenance 📝
- Counts TODO/FIXME/XXX/HACK comments (target: ≤5)

### 4. Formatting 🎨
- Runs `cargo fmt --check`

### 5. Linting 🔍
- Runs `cargo clippy -- -D warnings`

### 6. Testing 🧪
- Calculates test coverage percentage (target: ≥60%)

### 7. Dependencies 📦
- Runs `cargo audit` if available

## Exit Codes

- `0` - All checks passed
- `1` - One or more checks failed

## Example Output

```
🔍 Running Roxy code quality checks...

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📦 Error Handling
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Checking for .unwrap() in production code... ✓ Found 12 occurrences
Checking for .expect() in production code... ✓ Found 8 occurrences

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
🔐 Security
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Checking for SQL injection patterns... ✓
Checking for potential credential leaks... ✓

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📝 Code Maintenance
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Checking for TODO comments... ✓ Found 3 TODO comments

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
🎨 Code Formatting
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Checking formatting... ✓

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
🔍 Linting
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Running clippy lints... ✓

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
🧪 Testing
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Checking test coverage... ✓ 65% of files have tests

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📦 Dependencies
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Checking for security vulnerabilities... ✓

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📊 Summary
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
✓ All checks passed!
```

## Integration with CI/CD

Add to your CI workflow:

```yaml
- name: Run quality checks
  run: cargo xtask check
```

## Extending

To add new checks, edit `xtask/src/main.rs`:

```rust
// Add a new check function
fn check_something_new() -> Result<()> {
    print!("Checking something new... ");
    // Your check logic here
    println!("{}", "✓".green().bold());
    Ok(())
}

// Call it from run_quality_checks()
fn run_quality_checks() -> Result<()> {
    // ... existing checks
    
    if let Err(e) = check_something_new() {
        println!("{} {}", "✗".red().bold(), e);
        failures += 1;
    }
    
    // ... rest of function
}
```

## Dependencies

- `walkdir` - For traversing the file tree
- `regex` - For pattern matching in files
- `colored` - For colored terminal output
- `anyhow` - For error handling

All dependencies are development-only and won't affect the production binaries.
