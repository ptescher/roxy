# Roxy Proxy

A fast, efficient, local network debugging tool built with Rust.

## Overview

Roxy is a modern alternative to tools like Proxyman or Charles Proxy. It intercepts HTTP/HTTPS traffic from your local machine, instruments it with OpenTelemetry traces, stores everything in ClickHouse, and displays it in a native GPU-accelerated UI.

**Key Features:**
- 🚀 Fast HTTP/HTTPS proxy with TLS interception (MITM)
- 📊 OpenTelemetry-native tracing and metrics
- 💾 ClickHouse storage for efficient querying of historical data
- 🖥️ Native macOS UI built with GPUI (Zed's UI framework)
- 🔧 Automatic system proxy configuration on macOS
- 📦 **Fully self-contained** - no Docker required! All services are downloaded and managed automatically
- 🔄 **Auto-updates** - checks for new versions and can update itself
- 🎯 **Visual routing rules** - redirect specific API paths to local servers via the GUI
- ☸️ **Kubernetes integration** - manage port forwards and route K8s DNS names through the UI

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Roxy UI (GPUI)                          │
│                                                                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
│  │   Sidebar   │  │  Request    │  │     Detail Panel        │ │
│  │   (Hosts)   │  │    List     │  │  (Headers/Body/Timing)  │ │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘ │
│  ┌─────────────┐  ┌─────────────────────────────────────────┐  │
│  │  Routing    │  │           Kubernetes                     │  │
│  │   Rules     │  │         Port Forwards                    │  │
│  └─────────────┘  └─────────────────────────────────────────┘  │
└────────────────────────────┬────────────────────────────────────┘
                             │ Spawns & manages subprocess
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Roxy Proxy Worker                          │
│                       (Port 8080)                               │
│                                                                 │
│   ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐   │
│   │ HTTP/HTTPS  │───▶│  Routing    │───▶│   Forward to    │   │
│   │   Proxy     │    │   Engine    │    │  Local/Remote   │   │
│   └─────────────┘    └──────┬──────┘    └─────────────────┘   │
│                             │                                   │
│                    Manages subprocesses                         │
└─────────────────────────────┼───────────────────────────────────┘
                              │
           ┌──────────────────┴──────────────────┐
           ▼                                     ▼
┌─────────────────────┐              ┌─────────────────────┐
│    ClickHouse       │◀─────────────│   OTel Collector    │
│   (Port 8123)       │   Exports    │   (Port 4317)       │
│                     │   traces     │                     │
│  Auto-downloaded    │              │  Auto-downloaded    │
│  & managed          │              │  & managed          │
└─────────────────────┘              └─────────────────────┘
```

### Components

| Component | Description | Port |
|-----------|-------------|------|
| `roxy` (UI) | Native macOS application, spawns and manages the proxy | - |
| `roxy-proxy` | HTTP/HTTPS proxy with TLS interception | 8080 |
| `roxy-core` | Shared library: types, service management, auto-updater | - |
| ClickHouse | Time-series database (auto-managed subprocess) | 8123, 9000 |
| OTel Collector | Telemetry processing (auto-managed subprocess) | 4317, 4318 |

### How It Works

When you launch Roxy:

1. **UI starts** and creates the main window
2. **UI spawns `roxy-proxy`** as a managed subprocess
3. **Proxy checks for services** - if ClickHouse or OTel Collector aren't installed:
   - Downloads binaries for your platform from GitHub releases
   - Generates configuration files in `~/.roxy/config/`
4. **Proxy starts services** - launches ClickHouse and OTel as subprocesses
5. **Proxy begins intercepting** - listens on port 8080 for HTTP/HTTPS traffic
6. **UI polls ClickHouse** - displays captured requests in real-time
7. **On quit** - everything shuts down gracefully in reverse order

All data is stored locally in `~/.roxy/`.

## Prerequisites

- **Rust** (1.75+ recommended) - [Install](https://rustup.rs/)
- **macOS** (for the GPUI-based UI)
- **Xcode Command Line Tools** with Metal Toolchain (for UI compilation):
  ```bash
  xcode-select --install
  xcodebuild -downloadComponent MetalToolchain
  ```

> **Note**: The proxy can run on Linux without the UI. See [Supported Platforms](#supported-platforms).

## Quick Start

### 1. Clone and Build

```bash
git clone https://github.com/yourusername/roxy.git
cd roxy

# Build all crates
cargo build --release
```

### 2. Run Roxy

```bash
# Run the UI (which will start the proxy automatically)
cargo run --release --bin roxy
```

On first run, you'll see:
```
INFO Starting Roxy UI v0.1.0
INFO Starting proxy subprocess...
INFO Installing ClickHouse...
INFO Downloading from: https://github.com/ClickHouse/ClickHouse/releases/...
INFO Download progress: 45.2% (180 MB / 398 MB)
...
INFO Installing OpenTelemetry Collector...
INFO Starting ClickHouse...
INFO ClickHouse is healthy
INFO Starting OpenTelemetry Collector...
INFO Proxy started successfully
```

### 3. Trust the CA Certificate

On first run, Roxy generates a self-signed CA certificate for TLS interception. To intercept HTTPS traffic:

1. Open **Keychain Access** on macOS
2. Import the certificate from `~/.roxy/ca.crt`
3. Double-click the certificate and set "Trust" to "Always Trust"

### 4. Configure Your Browser/App

Set your HTTP proxy to `127.0.0.1:8080`:

- **macOS System**: System Preferences → Network → Advanced → Proxies
- **Firefox**: Settings → Network Settings → Manual proxy configuration
- **curl**: `curl -x http://127.0.0.1:8080 https://example.com`

---

## Example: Full-Stack Development with API Routing & Kubernetes

This example demonstrates a real-world development workflow where you're building a new API endpoint while your mobile app still needs to talk to production services, and your backend needs to connect to a database running in Kubernetes.

### The Scenario

You're working on:
- **Frontend**: Expo React Native app (running with hot reload)
- **Backend**: NestJS API server (running locally on port 3000 with hot reload)
- **Production API**: `api.example.app`
- **Database**: PostgreSQL in Kubernetes (`postgres-primary.backend.svc.cluster.local`)

**What you want:**
- Calls to `api.example.app/v1/new-endpoint/*` → your local NestJS server (port 3000)
- Calls to `api.example.app/v1/*` (everything else) → production API
- Your NestJS server connects to `postgres-primary.backend.svc.cluster.local:5432` → routed to Kubernetes via port-forward

**All configured through the Roxy GUI—no config files needed!**

---

### Step 1: Start Roxy and Open the Routing Panel

Launch Roxy:
```bash
cargo run --release --bin roxy
```

In the Roxy window, click the **"Routing Rules"** tab in the sidebar.

---

### Step 2: Set Up Kubernetes Port Forwarding (via GUI)

Click the **"Kubernetes"** tab in the sidebar. You'll see the Kubernetes panel.

**Connect to your cluster:**
1. Click **"Add Cluster"**
2. Roxy automatically detects your `~/.kube/config`
3. Select your context (e.g., `my-staging-cluster`)
4. Click **"Connect"**

**Create the PostgreSQL port forward:**
1. Click **"New Port Forward"**
2. Fill in the form:
   - **Name**: `Postgres Primary`
   - **Namespace**: `backend`
   - **Service**: `postgres-primary` (Roxy lists available services)
   - **Remote Port**: `5432`
   - **Local Port**: `5432`
   - ✅ **Auto-start**: Check this to start on launch
   - ✅ **Create routing rule**: Check this to auto-route the K8s DNS name
3. Click **"Create & Start"**

Roxy will:
- Run `kubectl port-forward -n backend svc/postgres-primary 5432:5432` in the background
- Automatically create a routing rule so `postgres-primary.backend.svc.cluster.local:5432` routes to `localhost:5432`

You'll see the port forward status turn green: **● Postgres Primary (5432 → 5432)**

---

### Step 3: Create Routing Rules for Your API

Click back to the **"Routing Rules"** tab.

**Rule 1: Route new endpoint to local development server**

1. Click **"New Rule"**
2. Configure the rule:
   - **Name**: `New Endpoint → Local`
   - **Priority**: `10` (lower = higher priority)
   
   **Match Conditions:**
   - **Host**: `api.example.app`
   - **Path**: Starts with `/v1/new-endpoint`
   
   **Action**: Forward to Local
   - **Local Port**: `3000`
   - ✅ **Preserve Host Header**: Keep checked so your backend sees the original host

3. Click **"Create Rule"**

**Rule 2: Everything else goes to production**

1. Click **"New Rule"**
2. Configure:
   - **Name**: `Other API → Production`
   - **Priority**: `100` (lower priority, evaluated after rule 1)
   
   **Match Conditions:**
   - **Host**: `api.example.app`
   
   **Action**: Passthrough (forward to original destination)

3. Click **"Create Rule"**

Your rules panel should now show:

```
┌─────────────────────────────────────────────────────────────┐
│ Routing Rules                                        [+ New] │
├─────────────────────────────────────────────────────────────┤
│ ✅ 10  New Endpoint → Local                                 │
│        api.example.app/v1/new-endpoint/* → localhost:3000   │
│                                                             │
│ ✅ 100 Other API → Production                               │
│        api.example.app/* → passthrough                      │
│                                                             │
│ ✅ --  K8s: Postgres Primary (auto-generated)               │
│        postgres-primary.backend.svc.cluster.local:5432      │
│        → localhost:5432                                     │
└─────────────────────────────────────────────────────────────┘
```

---

### Step 4: Configure Your Development Servers

**NestJS Backend** (`backend/`)

Your NestJS app connects to the database using the Kubernetes DNS name. Roxy intercepts this and routes it to your port-forward:

```typescript
// src/app.module.ts
import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';

@Module({
  imports: [
    TypeOrmModule.forRoot({
      type: 'postgres',
      // Use the Kubernetes DNS name - Roxy routes this to localhost:5432
      host: 'postgres-primary.backend.svc.cluster.local',
      port: 5432,
      username: process.env.DB_USER,
      password: process.env.DB_PASSWORD,
      database: 'myapp',
      ssl: false, // Disable SSL for local development
    }),
  ],
})
export class AppModule {}
```

Start NestJS with the proxy configured:

```bash
cd backend

# Set proxy environment variables so Node.js uses Roxy
HTTP_PROXY=http://localhost:8080 \
HTTPS_PROXY=http://localhost:8080 \
npm run start:dev
```

**Expo React Native App** (`mobile/`)

Your React Native app calls the production API URL. Roxy intercepts and routes based on your rules:

```typescript
// src/api/client.ts
const API_BASE = 'https://api.example.app';

export async function fetchNewEndpoint() {
  // This goes to localhost:3000 (your local NestJS)
  const response = await fetch(`${API_BASE}/v1/new-endpoint/data`);
  return response.json();
}

export async function fetchUsers() {
  // This goes to production api.example.app
  const response = await fetch(`${API_BASE}/v1/users`);
  return response.json();
}
```

Configure the iOS Simulator to use the proxy (or use Expo's proxy settings):

```bash
cd mobile

# For iOS Simulator - configure system proxy, or:
npx expo start --ios
```

For Android Emulator:
```bash
# Route emulator traffic through host proxy
adb reverse tcp:8080 tcp:8080
```

---

### Step 5: See It All in Action

With everything running:
- NestJS on port 3000
- Expo dev server
- Roxy with port-forward and routing rules

Open your React Native app and trigger some API calls. In Roxy's main traffic view, you'll see:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ Hosts                    │ Network Traffic                                  │
│                          │                                                  │
│ ● api.example.app   (47) │ Method  Status  URL                    Duration │
│ ● postgres-primar…  (12) │                                                  │
│                          │ POST    201     /v1/new-endpoint/data   45ms    │
│ Routing Rules       (3)  │         └─ → localhost:3000 (local NestJS)      │
│ K8s Port Forwards   (1)  │                                                  │
│                          │ GET     200     /v1/users               120ms   │
│                          │         └─ → api.example.app (production)       │
│                          │                                                  │
│                          │ GET     200     /v1/new-endpoint/items  38ms    │
│                          │         └─ → localhost:3000 (local NestJS)      │
│                          │                                                  │
│                          │ TCP     OK      postgres-primary.back…  2ms     │
│                          │         └─ → localhost:5432 (K8s port-forward)  │
│                          │                                                  │
│                          │ GET     200     /v1/products            95ms    │
│                          │         └─ → api.example.app (production)       │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Click any request to see:**
- Full request/response headers
- Request and response bodies (formatted JSON/XML)
- Timing breakdown (DNS, connect, TLS, TTFB, transfer)
- Which routing rule matched
- OpenTelemetry trace ID for distributed tracing

---

### Step 6: Advanced Routing (Conditional Rules)

Need more complex routing? Roxy supports matching on headers, query params, and more—all via the GUI.

**Example: Route beta users to local**

1. Click **"New Rule"**
2. Configure:
   - **Name**: `Beta Users → Local`
   - **Priority**: `5` (higher than the path-based rule)
   
   **Match Conditions:**
   - **Host**: `api.example.app`
   - Click **"+ Add Header Match"**
   - **Header Name**: `X-Beta-User`
   - **Header Value**: `true`
   
   **Action**: Forward to Local
   - **Local Port**: `3000`

Now requests with `X-Beta-User: true` header go to your local server, regardless of path.

**Example: Route specific user for debugging**

Create a rule matching:
- **Header**: `X-User-ID` equals `12345`
- **Action**: Forward to Local with **additional header** `X-Debug-Mode: true`

---

### Saving and Sharing Your Configuration

Your routing rules and Kubernetes settings are automatically saved as a **Project** in Roxy.

**To export your project:**
1. Click **File → Export Project**
2. Save as `my-project.roxy`

**To import on another machine:**
1. Click **File → Import Project**
2. Select the `.roxy` file
3. All rules and K8s configurations are restored

This makes it easy to share development configurations with your team!

---

## Auto-Updates

Roxy includes a built-in auto-update system that:

- Checks for new versions on GitHub Releases at startup
- Shows an update notification banner in the UI when available
- Can download and apply updates automatically
- Supports update channels: `stable`, `beta`, `nightly`

### Update Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `ROXY_UPDATE_CHANNEL` | Update channel (`stable`, `beta`, `nightly`) | `stable` |
| `ROXY_UPDATE_CHECK` | Enable/disable update checks | `true` |

The updater also handles updating the managed services (ClickHouse, OTel Collector) when new versions are available.

---

## Data Directory Structure

All Roxy data is stored in `~/.roxy/`:

```
~/.roxy/
├── bin/                    # Downloaded binaries
│   ├── clickhouse          # ClickHouse server
│   ├── otelcol-contrib     # OpenTelemetry Collector
│   └── roxy-proxy          # Proxy binary (when updated)
├── config/                 # Service configurations
│   ├── clickhouse-config.xml
│   ├── clickhouse-users.xml
│   └── otel-collector.yaml
├── projects/               # Saved projects with routing rules
│   └── default.json
├── data/                   # Persistent data
│   └── clickhouse/         # ClickHouse database files
├── logs/                   # Service logs
│   ├── clickhouse.log
│   ├── clickhouse-error.log
│   ├── otel-collector-stdout.log
│   └── otel-collector-stderr.log
├── updates/                # Downloaded updates (temporary)
└── ca.crt                  # Generated CA certificate
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ROXY_PROXY_PORT` | Proxy listen port | `8080` |
| `ROXY_START_SERVICES` | Start ClickHouse/OTel automatically | `true` |
| `ROXY_CONFIGURE_SYSTEM_PROXY` | Auto-configure macOS system proxy | `false` |
| `RUST_LOG` | Logging level | `info,roxy_proxy=debug` |

### Running Proxy Standalone

If you want to run just the proxy (e.g., on Linux without the UI):

```bash
cargo run --release --bin roxy-proxy
```

### Running with External Services

If you want to run ClickHouse and OTel Collector separately:

```bash
ROXY_START_SERVICES=false cargo run --release --bin roxy-proxy
```

## Development

### Project Structure

```
roxy/
├── Cargo.toml                  # Workspace configuration
├── README.md
└── crates/
    ├── roxy-core/              # Shared library
    │   └── src/
    │       ├── lib.rs          # Public exports
    │       ├── models.rs       # HTTP request/response types
    │       ├── clickhouse.rs   # ClickHouse client & schema
    │       ├── routing.rs      # Routing rules & K8s integration
    │       ├── proxy_manager.rs # Proxy subprocess management
    │       ├── updater.rs      # Auto-update system
    │       └── services/       # Backend service management
    │           ├── mod.rs
    │           ├── downloader.rs   # Binary download manager
    │           ├── clickhouse.rs   # ClickHouse subprocess
    │           └── otel_collector.rs
    ├── roxy-proxy/             # Proxy worker binary
    │   └── src/
    │       └── main.rs
    └── roxy-ui/                # GPUI native UI
        └── src/
            └── main.rs
```

### Running Tests

```bash
# Run all tests
cargo test

# Run with logging
RUST_LOG=debug cargo test -- --nocapture

# Run specific crate tests
cargo test -p roxy-core
```

### Building for Release

```bash
# Build optimized binaries
cargo build --release

# Binaries will be in target/release/
ls -la target/release/roxy target/release/roxy-proxy
```

### Building macOS App Bundle

The project includes scripts for creating a proper macOS `.app` bundle with icon, code signing, and DMG creation:

```bash
# Generate app icons (one-time, requires librsvg: brew install librsvg)
./script/generate-icon

# Debug build - fast iteration, opens the app
./script/bundle-mac -d -o

# Release build - creates signed .app and .dmg
./script/bundle-mac

# Install to /Applications
./script/bundle-mac -i

# Build for specific architecture
./script/bundle-mac aarch64-apple-darwin  # Apple Silicon
./script/bundle-mac x86_64-apple-darwin   # Intel
```

For code signing and notarization (required for distribution), set these environment variables:
- `MACOS_SIGNING_IDENTITY` - Your Developer ID certificate name
- `MACOS_CERTIFICATE` - Base64-encoded .p12 certificate (for CI)
- `MACOS_CERTIFICATE_PASSWORD` - Certificate password
- `APPLE_NOTARIZATION_KEY`, `APPLE_NOTARIZATION_KEY_ID`, `APPLE_NOTARIZATION_ISSUER_ID` - For notarization

## Roadmap

### Backend (roxy-proxy)
- [x] Basic HTTP proxy with TLS interception
- [x] macOS system proxy configuration
- [x] Auto-download and manage ClickHouse
- [x] Auto-download and manage OTel Collector
- [x] Graceful subprocess management
- [x] Routing rule data model
- [x] Kubernetes port-forward data model
- [ ] Routing rule evaluation engine
- [ ] OpenTelemetry span creation for traced requests
- [ ] Store captured requests in ClickHouse via otel collector
- [ ] SOCKS5 proxy support
- [ ] Kubernetes service discovery
- [ ] OpenAPI schema awareness
- [ ] Request/response modification
- [ ] Breakpoints and debugging

### Frontend (roxy-ui)
- [x] Basic window and layout
- [x] Sidebar with host list
- [x] Request list view
- [x] Request detail panel
- [x] Spawn and manage proxy subprocess
- [x] Proxy status indicator
- [x] Update notification banner
- [x] Request row click to select and view details
- [x] Host sidebar click to filter requests
- [x] Set up proper Mac app with icon, "About Roxy", etc
- [ ] Break out front end into proper reactive architecute with separate views, components, etc
- [ ] Basic filtering (real time search)
- [ ] Routing rules panel (create/edit/delete)
- [ ] Kubernetes panel (clusters, port-forwards)
- [ ] Real-time updates from ClickHouse
- [ ] Request filtering and search
- [ ] Trace waterfall visualization
- [ ] Request/response body formatting (JSON, XML, etc.)
- [ ] Host metrics dashboard (RPS, latency, errors)
- [ ] Project import/export
- [ ] Dark/light theme toggle

### Infrastructure
- [x] Auto-download service binaries
- [x] Platform detection (macOS, Linux)
- [x] Update checking from GitHub releases
- [ ] Automatic update installation
- [ ] Crash reporting (opt-in)
- [ ] Telemetry (opt-in)

## Troubleshooting

### Proxy not intercepting traffic

1. Check the proxy is running: `curl -x http://localhost:8080 http://example.com`
2. Verify system proxy settings: **System Preferences → Network → Advanced → Proxies**
3. For HTTPS, ensure the CA certificate is trusted in Keychain Access

### Services fail to start

1. Check logs in `~/.roxy/logs/`
2. Verify ports aren't in use: `lsof -i :8123` and `lsof -i :4317`
3. Delete `~/.roxy/bin/` to force re-download of binaries
4. Check available disk space (ClickHouse needs ~500MB)

### Kubernetes port-forward not working

1. Verify `kubectl` is in your PATH
2. Check you're connected to the right cluster context
3. Verify the service exists: `kubectl get svc -n <namespace>`
4. Check the port-forward logs in Roxy's Kubernetes panel

### Routing rules not matching

1. Check rule priority (lower number = higher priority)
2. Verify the host and path patterns match exactly
3. Use the "Test Match" button in the rule editor to debug
4. Check the request detail view to see which rule (if any) matched

### Download fails

If binary download fails:
1. Check your internet connection
2. Try manually downloading from the URLs shown in the logs
3. Place binaries in `~/.roxy/bin/` with names `clickhouse` and `otelcol-contrib`
4. Make them executable: `chmod +x ~/.roxy/bin/*`

### UI not showing data

1. Ensure the proxy is running (check status indicator in sidebar)
2. Check ClickHouse has data: `curl "http://localhost:8123/?query=SELECT+count()+FROM+roxy.http_requests"`
3. Look for errors in `~/.roxy/logs/`

### Metal shader compilation fails

If you see "cannot execute tool 'metal'":
```bash
xcodebuild -downloadComponent MetalToolchain
```

## Supported Platforms

| Platform | UI | Proxy | Backend Services |
|----------|-----|-------|------------------|
| macOS (Apple Silicon) | ✅ | ✅ | ✅ |
| macOS (Intel) | ✅ | ✅ | ✅ |
| Linux (x86_64) | ❌ | ✅ | ✅ |
| Linux (ARM64) | ❌ | ✅ | ✅ |
| Windows | ❌ | ❌ | ❌ |

> **Note**: GPUI currently only supports macOS. The proxy can run on Linux for headless/server use cases.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [GPUI](https://github.com/zed-industries/zed) - GPU-accelerated UI framework by the Zed team
- [hudsucker](https://github.com/omjadas/hudsucker) - Rust HTTP/HTTPS proxy library
- [OpenTelemetry](https://opentelemetry.io/) - Observability framework
- [ClickHouse](https://clickhouse.com/) - Fast analytics database
