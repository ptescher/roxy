# Security Review Skill

This skill performs security-focused code review for the Roxy codebase, with emphasis on the security-critical nature of a TLS-intercepting proxy.

## When to Use

Activate this skill when:
- Reviewing TLS/cryptography code
- Reviewing protocol parsers
- Reviewing authentication/authorization code
- Adding new network-facing functionality
- User asks for a security review

## Threat Model

Roxy operates with elevated privileges (TLS interception), so security is critical:

### Trust Boundaries

1. **User ↔ Roxy:** Full trust - user trusts Roxy with plaintext HTTPS traffic
2. **Roxy ↔ Remote Servers:** TLS encryption (Roxy acts as MITM)
3. **Roxy ↔ ClickHouse:** Localhost connection (trusted)
4. **Roxy ↔ Kubernetes API:** Uses kubeconfig credentials (trusted)

### Threat Scenarios

1. **CA Private Key Compromise** - Attacker gains access to `~/.roxy/ca.key`
2. **Protocol Parser Vulnerabilities** - Malformed packets crash/exploit Roxy
3. **SQL Injection** - Malicious headers injected into ClickHouse queries
4. **Resource Exhaustion** - Attacker floods proxy with connections
5. **Information Disclosure** - Sensitive data logged or leaked

## Security Checklist

### 1. Cryptography & TLS 🔐

**Check for:**
- [ ] No hardcoded keys or passwords
- [ ] CA private key has 0600 permissions
- [ ] Certificate generation uses strong parameters (RSA 2048+, or Ed25519)
- [ ] TLS configuration uses secure ciphersuites
- [ ] No insecure TLS versions (SSLv3, TLS 1.0, TLS 1.1)

**Example - BAD:**
```rust
// ❌ Insecure: Hardcoded password
const CA_PASSWORD: &str = "changeme";

// ❌ Insecure: Weak RSA key
let keypair = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
```

**Example - GOOD:**
```rust
// ✅ Generate strong key
use rcgen::{Certificate, CertificateParams, KeyPair};
use rcgen::SignatureAlgorithm;

let mut params = CertificateParams::new(vec!["localhost".into()]);
params.alg = &SignatureAlgorithm::PKCS_ECDSA_P256_SHA256;  // Or Ed25519
let keypair = KeyPair::generate(&params.alg)?;

// ✅ Store with secure permissions
let ca_key_path = dirs::home_dir().unwrap().join(".roxy/ca.key");
std::fs::write(&ca_key_path, keypair.serialize_pem())?;
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&ca_key_path)?.permissions();
    perms.set_mode(0o600);  // Owner read/write only
    std::fs::set_permissions(&ca_key_path, perms)?;
}
```

**TLS Configuration:**
```rust
// ✅ Secure TLS configuration
let mut root_store = rustls::RootCertStore::empty();
root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

let config = rustls::ClientConfig::builder()
    .with_root_certificates(root_store)
    .with_no_client_auth();

// Ensure secure ciphersuites (rustls uses secure defaults)
```

### 2. Input Validation 🛡️

**Check for:**
- [ ] All protocol parsers validate message lengths
- [ ] Buffer bounds are checked before access
- [ ] No unsafe arithmetic (overflow, underflow)
- [ ] String lengths are validated
- [ ] No unrestricted memory allocation

**Example - BAD:**
```rust
// ❌ Unchecked allocation - DoS vulnerability
fn parse_message(data: &[u8]) -> Result<Vec<u8>, Error> {
    let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let mut buffer = vec![0u8; length as usize];  // Could allocate 4GB!
    // ...
}
```

**Example - GOOD:**
```rust
// ✅ Validated allocation with limits
const MAX_MESSAGE_SIZE: u32 = 256 * 1024 * 1024;  // 256 MB
const MAX_ALLOCATION_SIZE: usize = 64 * 1024 * 1024;  // 64 MB

fn parse_message(data: &[u8]) -> Result<Vec<u8>, Error> {
    if data.len() < 4 {
        return Err(Error::Incomplete);
    }
    
    let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    
    // Validate length against maximum
    if length > MAX_MESSAGE_SIZE {
        return Err(Error::MessageTooLarge(length));
    }
    
    // Check for integer overflow
    let alloc_size = length.checked_sub(4)
        .ok_or(Error::InvalidLength)?
        as usize;
    
    if alloc_size > MAX_ALLOCATION_SIZE {
        return Err(Error::AllocationLimitExceeded);
    }
    
    // Safe to allocate
    let mut buffer = vec![0u8; alloc_size];
    // ...
}
```

### 3. SQL Injection Prevention 🔴 CRITICAL

**Check for:**
- [ ] NO string formatting for SQL queries
- [ ] All queries use parameterized statements (`.bind()`)
- [ ] No manual escaping functions
- [ ] No raw SQL string concatenation

**Example - DANGEROUS:**
```rust
// 🔴 CRITICAL: SQL Injection vulnerability
pub async fn get_requests_by_host(&self, host: &str) -> Result<Vec<HttpRequestRecord>> {
    let query = format!(
        "SELECT * FROM http_requests WHERE host = '{}'",
        host.replace("'", "\\'")  // ❌ Insufficient escaping
    );
    self.client.query(&query).fetch_all().await
}

// Attack: host = "'; DROP TABLE http_requests; --"
```

**Example - SAFE:**
```rust
// ✅ Safe: Parameterized query
pub async fn get_requests_by_host(&self, host: &str) -> Result<Vec<HttpRequestRecord>> {
    self.client
        .query("SELECT * FROM http_requests WHERE host = ?")
        .bind(host)  // ✅ Safely parameterized
        .fetch_all()
        .await
        .map_err(Into::into)
}
```

### 4. Authentication & Authorization 🔑

**Check for:**
- [ ] Kubernetes credentials are loaded from kubeconfig (not hardcoded)
- [ ] No credentials in logs or error messages
- [ ] Sensitive data is not included in spans/traces
- [ ] No authentication bypass vulnerabilities

**Example - BAD:**
```rust
// ❌ Leaking credentials in logs
tracing::info!("Connecting to K8s with token: {}", kube_token);

// ❌ Error includes sensitive data
Err(anyhow::anyhow!("Failed to authenticate with password: {}", password))
```

**Example - GOOD:**
```rust
// ✅ Redact credentials in logs
tracing::info!("Connecting to K8s cluster: {}", cluster_url);

// ✅ Generic error without sensitive data
Err(anyhow::anyhow!("Failed to authenticate - check credentials"))

// ✅ Redact in structured logs
#[derive(Debug)]
struct SanitizedConfig {
    url: String,
    #[allow(dead_code)]
    token: String,  // Not included in Debug output
}

impl std::fmt::Display for SanitizedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Config {{ url: {}, token: <redacted> }}", self.url)
    }
}
```

### 5. Resource Exhaustion 💥

**Check for:**
- [ ] Connection limits enforced
- [ ] Request size limits enforced
- [ ] Timeout on all network operations
- [ ] No unbounded queues or buffers
- [ ] Rate limiting on expensive operations

**Example - BAD:**
```rust
// ❌ Unbounded connection acceptance
loop {
    let (stream, addr) = listener.accept().await?;
    tokio::spawn(handle_connection(stream, addr));  // No limit!
}
```

**Example - GOOD:**
```rust
// ✅ Bounded connection pool
use tokio::sync::Semaphore;

const MAX_CONCURRENT_CONNECTIONS: usize = 10_000;

let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));

loop {
    let permit = semaphore.clone().acquire_owned().await?;
    let (stream, addr) = listener.accept().await?;
    
    tokio::spawn(async move {
        let _permit = permit;  // Held until task completes
        handle_connection(stream, addr).await;
    });
}

// ✅ Request size limits
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;  // 10 MB

async fn read_body(body: hyper::Body) -> Result<Bytes, Error> {
    let bytes = body
        .collect()
        .await
        .map_err(|_| Error::BodyReadFailed)?
        .to_bytes();
    
    if bytes.len() > MAX_BODY_SIZE {
        return Err(Error::BodyTooLarge);
    }
    
    Ok(bytes)
}

// ✅ Timeouts on all operations
async fn connect_with_timeout(addr: &str) -> Result<TcpStream, Error> {
    tokio::time::timeout(
        Duration::from_secs(30),
        TcpStream::connect(addr)
    )
    .await
    .map_err(|_| Error::Timeout)?
    .map_err(Into::into)
}
```

### 6. Information Disclosure 🕵️

**Check for:**
- [ ] Sensitive headers not logged (Authorization, Cookie, etc.)
- [ ] Request bodies sanitized before storage
- [ ] Error messages don't leak internal paths
- [ ] Stack traces not sent to client

**Example - BAD:**
```rust
// ❌ Logs sensitive headers
fn log_request(req: &Request) {
    tracing::info!("Request: {:?}", req.headers());  // Includes Authorization!
}

// ❌ Leaks internal paths
return Err(anyhow::anyhow!(
    "Failed to open /Users/admin/.roxy/ca.key: {}",
    e
));
```

**Example - GOOD:**
```rust
// ✅ Sanitize sensitive headers
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "proxy-authorization",
    "x-api-key",
];

fn sanitize_headers(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| {
            let key = k.as_str().to_lowercase();
            let value = if SENSITIVE_HEADERS.contains(&key.as_str()) {
                "<redacted>".to_string()
            } else {
                v.to_str().unwrap_or("<binary>").to_string()
            };
            (k.as_str().to_string(), value)
        })
        .collect()
}

// ✅ Generic error messages
return Err(anyhow::anyhow!("Failed to load CA certificate - check file permissions"));
```

### 7. Unsafe Code 🚨

**Check for:**
- [ ] All `unsafe` blocks have safety comments
- [ ] `unsafe` is justified and cannot be replaced with safe code
- [ ] Memory safety invariants are documented
- [ ] No data races in `unsafe` code

**Example - NEEDS JUSTIFICATION:**
```rust
// ❌ Why is this unsafe? Is it necessary?
unsafe {
    let ptr = data.as_ptr();
    std::ptr::read(ptr)
}
```

**Example - GOOD:**
```rust
// ✅ Justified unsafe with safety comment
/// # Safety
/// This is safe because:
/// - We've verified `offset` is within bounds (checked above)
/// - The data buffer is properly aligned for u32
/// - The lifetime of the reference doesn't outlive `data`
unsafe {
    let ptr = data.as_ptr().add(offset) as *const u32;
    u32::from_be(*ptr)
}
```

## Security Testing

### Fuzzing

Use `cargo-fuzz` to fuzz protocol parsers:

```rust
// fuzz/fuzz_targets/parse_postgres.rs

#![no_main]
use libfuzzer_sys::fuzz_target;
use roxy_proxy::protocol::postgres::parse_message;

fuzz_target!(|data: &[u8]| {
    // Parser should never panic on any input
    let _ = parse_message(data, false);
    let _ = parse_message(data, true);
});
```

Run fuzzing:
```bash
cargo install cargo-fuzz
cargo fuzz run parse_postgres
```

### Penetration Testing

Create adversarial tests:

```rust
#[test]
fn test_sql_injection_attempt() {
    let malicious_host = "example.com'; DROP TABLE http_requests; --";
    let result = clickhouse.get_requests_by_host(malicious_host).await;
    
    // Should succeed without executing the DROP
    assert!(result.is_ok());
    
    // Verify table still exists
    let count: u64 = clickhouse.client
        .query("SELECT count() FROM http_requests")
        .fetch_one()
        .await
        .unwrap();
    assert!(count >= 0);  // Table exists
}

#[test]
fn test_header_injection() {
    let malicious_header = "test\r\nX-Injected-Header: malicious";
    // Verify headers are sanitized
}

#[test]
fn test_oversized_message() {
    let huge_message = vec![0u8; 1_000_000_000];  // 1 GB
    let result = parse_message(&huge_message, false);
    assert!(matches!(result, Err(ParseError::InvalidLength(_))));
}
```

## Security Documentation

Every security-sensitive function must document:

1. **Trust assumptions** - What input is trusted/untrusted?
2. **Validation** - What validation is performed?
3. **Side effects** - Does this function log/store sensitive data?

**Example:**
```rust
/// Parse a PostgreSQL wire protocol message from untrusted client input.
///
/// # Security Considerations
///
/// This function processes **untrusted network input** and must be resilient to:
/// - Malformed messages (returns `ParseError`)
/// - Oversized messages (enforces `MAX_MESSAGE_SIZE` limit)
/// - Malicious payloads (no unsafe code, all bounds checked)
///
/// The parser performs the following validation:
/// - Message length ≤ `MAX_MESSAGE_SIZE` (256 MB)
/// - String lengths are validated before allocation
/// - All array accesses are bounds-checked
///
/// # Privacy
///
/// This function does NOT log:
/// - SQL queries (may contain sensitive data)
/// - Parameter values (may contain PII)
/// - Connection credentials
///
/// # Panics
///
/// This function does not panic on any input. All errors are returned as `ParseError`.
pub fn parse_message(
    data: &[u8],
    is_startup: bool,
) -> Result<(PostgresMessage, usize), ParseError> {
    // ...
}
```

## CI/CD Security Checks

Add to `.github/workflows/security.yml`:

```yaml
name: Security

on: [push, pull_request]

jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Install cargo-audit
        run: cargo install cargo-audit
      - name: Run audit
        run: cargo audit
  
  clippy-security:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Run Clippy with security lints
        run: cargo clippy -- -D clippy::suspicious -D clippy::perf
  
  semgrep:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: returntocorp/semgrep-action@v1
        with:
          config: >-
            p/security-audit
            p/rust
```

## Responsible Disclosure

If you find a security vulnerability:

1. **Do NOT** open a public GitHub issue
2. **Do NOT** disclose publicly before a fix is released
3. Email security@roxy.dev with details
4. Allow 90 days for a fix before public disclosure

## Security Checklist for PRs

Before submitting:

- [ ] No hardcoded credentials or keys
- [ ] All user input is validated
- [ ] No SQL injection vulnerabilities
- [ ] Sensitive data is redacted in logs
- [ ] Resource limits are enforced
- [ ] TLS configuration is secure
- [ ] `cargo audit` passes
- [ ] Fuzzing (if applicable) passes
- [ ] Security-sensitive functions are documented

## Severity Levels

- 🔴 **Critical:** Remote code execution, authentication bypass, data exfiltration
- 🟠 **High:** SQL injection, XSS, CSRF, DoS
- 🟡 **Medium:** Information disclosure, weak crypto
- 🟢 **Low:** Best practice violations, minor issues
