# Roxy Code Quality Skills

This directory contains Claude Code skills that enforce quality standards for the Roxy codebase.

## Available Skills

### 1. [code_review.md](./code_review.md) - Comprehensive Code Review
Ensures all code changes meet quality standards before merging.

**Key Checks:**
- ✅ Error handling (no `.unwrap()` in production code)
- 🔐 SQL injection prevention
- 🛡️ Input validation
- 🧪 Testing requirements
- 📝 Documentation standards
- ⚡ Performance best practices
- 🔒 Concurrency safety

**When it runs:** On every code change, PR, or commit

### 2. [test_requirements.md](./test_requirements.md) - Testing Standards
Enforces comprehensive test coverage for all code.

**Key Requirements:**
- 80% line coverage minimum
- Unit tests for all functions
- Integration tests for all public APIs
- Property-based testing for parsers
- Concurrency tests for shared state
- Performance benchmarks for hot paths

**When it runs:** Before commits, during PR review

### 3. [security_review.md](./security_review.md) - Security Analysis
Performs security-focused review for a TLS-intercepting proxy.

**Key Concerns:**
- 🔐 Cryptography & TLS configuration
- 🛡️ Input validation & DoS prevention
- 🔴 SQL injection (CRITICAL)
- 🔑 Authentication & authorization
- 💥 Resource exhaustion
- 🕵️ Information disclosure
- 🚨 Unsafe code review

**When it runs:** On all security-critical code, TLS changes, protocol parsers

## Quick Start

### Running Code Quality Checks

```bash
# Run all automated checks
cargo xtask check

# Individual checks
cargo xtask fmt-check   # Check formatting
cargo xtask fmt         # Auto-format code
cargo xtask clippy      # Run lints
cargo xtask test        # Run all tests
```

### Before Committing

1. **Self-review** using the checklists in each skill
2. **Run automated checks:** `cargo xtask check`
3. **Add tests** for new functionality
4. **Update documentation** if APIs changed

### During PR Review

Claude Code will automatically apply these skills when reviewing your PR. Address all feedback before merging.

## Severity Levels

Skills categorize issues by severity:

- 🔴 **Blocker** - Must fix (security, correctness)
- 🟠 **Major** - Should fix (error handling, testing)
- 🟡 **Minor** - Fix in follow-up (style, improvements)
- 🟢 **Suggestion** - Optional (optimizations, idioms)

## Integration with CI/CD

These skills align with our CI/CD pipeline:

```yaml
# .github/workflows/ci.yml
- Format check (cargo fmt)
- Clippy lints (cargo clippy)
- Tests (cargo test)
- Security audit (cargo audit)
- Code coverage (cargo tarpaulin)
```

All checks must pass before merging to `main`.

## Customizing Skills

To modify or add skills:

1. Edit the markdown file in `.claude/skills/`
2. Skills are automatically loaded by Claude Code
3. Test your changes by running code review

## Skill Usage Examples

### Example 1: Code Review for New Feature

```bash
# User: "Review my new K8s port-forwarding code"
# Claude will:
1. Check error handling (no .unwrap())
2. Verify input validation (DNS name parsing)
3. Check for resource leaks (port cleanup)
4. Ensure tests exist
5. Verify documentation
```

### Example 2: Security Review for Protocol Parser

```bash
# User: "Review my Kafka parser changes"
# Claude will:
1. Check for buffer overflows
2. Verify message size limits
3. Check for integer overflows
4. Ensure fuzzing tests exist
5. Verify no unsafe code
```

### Example 3: Test Coverage Check

```bash
# User: "What tests should I write for this module?"
# Claude will:
1. Analyze the code
2. Identify edge cases
3. Generate test templates
4. Check for missing error path tests
5. Suggest property-based tests
```

## Common Issues and Fixes

### Issue: Too many `.unwrap()` calls

**Problem:**
```rust
let config = load_config().unwrap();
```

**Fix:**
```rust
let config = load_config()
    .context("Failed to load configuration")?;
```

### Issue: SQL injection risk

**Problem:**
```rust
let query = format!("SELECT * FROM users WHERE id = '{}'", user_id);
```

**Fix:**
```rust
client.query("SELECT * FROM users WHERE id = ?")
    .bind(user_id)
    .fetch_all()
    .await?
```

### Issue: Missing input validation

**Problem:**
```rust
let length = u32::from_be_bytes(data[0..4].try_into()?);
let buffer = vec![0u8; length as usize];  // Could allocate GBs!
```

**Fix:**
```rust
const MAX_SIZE: u32 = 256 * 1024 * 1024;
let length = u32::from_be_bytes(data[0..4].try_into()?);
if length > MAX_SIZE {
    return Err(Error::MessageTooLarge);
}
let buffer = vec![0u8; length as usize];
```

## Metrics and Goals

### Current State (as of Jan 2026)

| Metric | Current | Target |
|--------|---------|--------|
| `.unwrap()` count | 136 | ≤20 |
| `.expect()` count | ~60 | ≤15 |
| Test coverage | ~40% | ≥80% |
| Files with tests | 21/50 | ≥45/50 |
| SQL injection risks | 1 | 0 |
| Large functions (>150 lines) | ~8 | ≤3 |

### Improvement Plan

**Week 1-2:**
- [ ] Fix SQL injection in `clickhouse.rs` (P0)
- [ ] Replace `.unwrap()` in hot paths (P1)
- [ ] Add integration tests (P1)

**Week 3-4:**
- [ ] Add input validation to parsers (P1)
- [ ] Increase test coverage to 60% (P2)
- [ ] Refactor large functions (P2)

**Month 2:**
- [ ] Achieve 80% test coverage (P2)
- [ ] Add performance benchmarks (P3)
- [ ] Set up continuous fuzzing (P3)

## Questions?

If you have questions about these skills or need clarification:

1. Check the detailed explanations in each skill file
2. Review the examples in this README
3. Ask Claude Code: "Explain the error handling requirements"
4. Consult `CODE_REVIEW.md` for the full review report

## Contributing

To improve these skills:

1. Identify a gap or issue
2. Update the relevant skill file
3. Add examples and rationale
4. Test with Claude Code
5. Submit a PR with your changes

---

**Remember:** These skills exist to help us write better, safer code. They're not meant to be burdensome, but to catch issues early before they become problems in production.

Happy coding! 🚀
