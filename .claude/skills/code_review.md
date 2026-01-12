# Code Review Skill

This skill ensures that all code changes meet the quality standards defined in CODE_REVIEW.md.

## When to Use

Automatically activate this skill when:
- A pull request is created
- Code is about to be committed
- User asks for a code review

## Review Checklist

### 1. Error Handling ✅

**Check for:**
- [ ] No `.unwrap()` calls in production code (use `.context()` or `?`)
- [ ] No `.expect()` calls without clear justification in comments
- [ ] All `Result` types are properly propagated with `?`
- [ ] Errors have meaningful context messages

**Example - BAD:**
```rust
let config = serde_json::from_str(&json).unwrap();
let client = Client::new().expect("Failed to create client");
```

**Example - GOOD:**
```rust
let config = serde_json::from_str(&json)
    .context("Failed to parse configuration JSON")?;
let client = Client::new()
    .context("Failed to create HTTP client - check network connectivity")?;
```

### 2. SQL Injection Prevention 🔐

**Check for:**
- [ ] No string formatting or concatenation for SQL queries
- [ ] All queries use `.bind()` for parameters
- [ ] No use of `escape_string()` or manual escaping

**Example - BAD:**
```rust
let query = format!("SELECT * FROM users WHERE id = '{}'", user_id);
client.query(&query).execute().await?;
```

**Example - GOOD:**
```rust
client.query("SELECT * FROM users WHERE id = ?")
    .bind(user_id)
    .fetch_all()
    .await?;
```

### 3. Input Validation 🛡️

**Check for:**
- [ ] All protocol parsers have maximum size limits
- [ ] Length fields are validated before allocation
- [ ] Buffer bounds are checked before access

**Example - BAD:**
```rust
let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
let slice = &data[..length as usize];  // Could panic or allocate gigabytes
```

**Example - GOOD:**
```rust
const MAX_MESSAGE_SIZE: u32 = 256 * 1024 * 1024;  // 256 MB

let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
if length > MAX_MESSAGE_SIZE {
    return Err(ParseError::InvalidLength(length));
}
if data.len() < length as usize {
    return Err(ParseError::Incomplete { needed: length as usize - data.len() });
}
let slice = &data[..length as usize];
```

### 4. Testing Requirements 🧪

**Check for:**
- [ ] All new functions have unit tests
- [ ] All new modules have integration tests
- [ ] Edge cases are covered (empty input, max size, error cases)
- [ ] Async functions use `#[tokio::test]`

**Example:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_valid_message() {
        let data = vec![/* ... */];
        let result = parse_message(&data);
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_parse_invalid_length() {
        let data = vec![0xFF, 0xFF, 0xFF, 0xFF];  // Huge length
        let result = parse_message(&data);
        assert!(matches!(result, Err(ParseError::InvalidLength(_))));
    }
    
    #[tokio::test]
    async fn test_async_operation() {
        let result = async_function().await;
        assert!(result.is_ok());
    }
}
```

### 5. Documentation 📝

**Check for:**
- [ ] All public functions have doc comments
- [ ] All modules have module-level docs
- [ ] Complex algorithms have inline comments
- [ ] Security considerations are documented

**Example:**
```rust
/// Parse a PostgreSQL wire protocol message
///
/// # Arguments
/// * `data` - Raw bytes from the TCP stream
/// * `is_startup` - Whether this is a startup-phase message
///
/// # Returns
/// Returns `Ok((message, bytes_consumed))` on success, or a `ParseError` if:
/// - The message is incomplete (needs more data)
/// - The message format is invalid
/// - The message size exceeds `MAX_MESSAGE_SIZE`
///
/// # Examples
/// ```
/// let data = vec![/* ... */];
/// let (msg, consumed) = parse_message(&data, false)?;
/// ```
pub fn parse_message(
    data: &[u8],
    is_startup: bool,
) -> Result<(PostgresMessage, usize), ParseError> {
    // ...
}
```

### 6. Performance ⚡

**Check for:**
- [ ] No unnecessary allocations in hot paths
- [ ] Collections pre-allocate capacity when size is known
- [ ] No blocking operations in async code
- [ ] Clones are justified (or use references)

**Example - BAD:**
```rust
async fn handle_request(req: Request) {
    std::thread::sleep(Duration::from_secs(1));  // Blocks the async runtime!
    
    let mut results = Vec::new();  // Will reallocate as it grows
    for i in 0..1000 {
        results.push(i);
    }
}
```

**Example - GOOD:**
```rust
async fn handle_request(req: Request) {
    tokio::time::sleep(Duration::from_secs(1)).await;  // Async sleep
    
    let mut results = Vec::with_capacity(1000);  // Pre-allocate
    for i in 0..1000 {
        results.push(i);
    }
}
```

### 7. Concurrency Safety 🔒

**Check for:**
- [ ] Shared state uses `Arc<RwLock<T>>` or `Arc<Mutex<T>>`
- [ ] Lock guards are dropped promptly (avoid holding across `.await`)
- [ ] No data races (use `Send + Sync` bounds appropriately)

**Example - BAD:**
```rust
async fn process_data(shared: Arc<Mutex<Data>>) {
    let mut data = shared.lock().await;
    some_async_operation().await;  // Lock held across await - deadlock risk!
    data.update();
}
```

**Example - GOOD:**
```rust
async fn process_data(shared: Arc<Mutex<Data>>) {
    let current = {
        let data = shared.lock().await;
        data.current_value()
    };  // Lock released here
    
    let updated = some_async_operation(current).await;
    
    {
        let mut data = shared.lock().await;
        data.update(updated);
    }  // Lock released
}
```

## Automated Checks

Run these commands before committing:

```bash
# Format code
cargo fmt --check

# Lint with Clippy
cargo clippy -- -D warnings

# Run tests
cargo test --all

# Check for common issues
./script/check_code_quality.sh
```

## Review Process

1. **Self-Review:**
   - Go through this checklist yourself before requesting review
   - Run automated checks
   - Add tests for new functionality

2. **Peer Review:**
   - Request review from at least one team member
   - Address all feedback
   - Re-run automated checks after changes

3. **Approval:**
   - All checks must pass
   - All comments must be resolved
   - No outstanding TODO comments without GitHub issues

## Severity Levels

- 🔴 **Blocker:** Must fix before merge (security, correctness)
- 🟠 **Major:** Should fix before merge (error handling, testing)
- 🟡 **Minor:** Fix in follow-up PR (style, minor improvements)
- 🟢 **Suggestion:** Optional improvement

## Examples of Issues by Severity

### 🔴 Blocker
- SQL injection vulnerability
- Unchecked buffer overflow
- Authentication bypass
- Unhandled panics in production code

### 🟠 Major
- Missing error handling (`.unwrap()` without justification)
- No tests for new functionality
- Race conditions
- Resource leaks

### 🟡 Minor
- Inconsistent naming
- Missing doc comments on public APIs
- Unnecessary clones
- Large functions (>150 lines)

### 🟢 Suggestion
- Could use `?` instead of `match`
- Consider caching this value
- This could be more idiomatic
