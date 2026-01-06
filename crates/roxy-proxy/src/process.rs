//! Process identification for finding which application is connecting to the proxy.
//!
//! Uses OS-level APIs to map a client socket (IP:port) to the originating process,
//! providing information like the process name, PID, and executable path.

use std::net::SocketAddr;
use std::path::PathBuf;

/// Information about a client process connecting to the proxy
#[derive(Debug, Clone, Default)]
pub struct ProcessInfo {
    /// Process ID
    pub pid: Option<i32>,
    /// Process name (e.g., "curl", "firefox")
    pub name: Option<String>,
    /// Full path to the executable
    pub path: Option<PathBuf>,
    /// Bundle identifier on macOS (e.g., "com.apple.Safari")
    pub bundle_id: Option<String>,
}

impl ProcessInfo {
    /// Get a display name for the process, preferring bundle_id > name > path > empty
    pub fn display_name(&self) -> String {
        if let Some(ref bundle_id) = self.bundle_id {
            return bundle_id.clone();
        }
        if let Some(ref name) = self.name {
            return name.clone();
        }
        if let Some(ref path) = self.path {
            if let Some(file_name) = path.file_name() {
                return file_name.to_string_lossy().to_string();
            }
        }
        String::new()
    }

    /// Check if we successfully identified the process
    pub fn is_identified(&self) -> bool {
        self.pid.is_some()
    }
}

/// Identify the process that owns a given client socket address.
///
/// This works by looking up which process has a socket bound to the client's
/// local port. Only works for local connections (127.0.0.1 or ::1).
pub fn identify_process(client_addr: &SocketAddr) -> ProcessInfo {
    // Only attempt identification for local connections
    if !client_addr.ip().is_loopback() {
        return ProcessInfo::default();
    }

    let client_port = client_addr.port();

    #[cfg(target_os = "macos")]
    {
        identify_process_macos(client_port)
    }

    #[cfg(target_os = "linux")]
    {
        identify_process_linux(client_port)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = client_port;
        ProcessInfo::default()
    }
}

#[cfg(target_os = "macos")]
fn identify_process_macos(client_port: u16) -> ProcessInfo {
    use libproc::libproc::bsd_info::BSDInfo;
    use libproc::libproc::file_info::{pidfdinfo, ListFDs, ProcFDType};
    use libproc::libproc::net_info::{SocketFDInfo, SocketInfoKind};
    use libproc::libproc::proc_pid::{listpidinfo, pidinfo};
    use libproc::processes::{pids_by_type, ProcFilter};

    // List all running processes
    let pids = match pids_by_type(ProcFilter::All) {
        Ok(pids) => pids,
        Err(_) => return ProcessInfo::default(),
    };

    // Check each process for a socket matching our client port
    for pid in pids {
        let pid = pid as i32;

        // Skip kernel processes
        if pid <= 0 {
            continue;
        }

        // Get process info to find number of file descriptors
        let info = match pidinfo::<BSDInfo>(pid, 0) {
            Ok(info) => info,
            Err(_) => continue,
        };

        // Get file descriptors for this process
        let fds = match listpidinfo::<ListFDs>(pid, info.pbi_nfiles as usize) {
            Ok(fds) => fds,
            Err(_) => continue,
        };

        // Check each file descriptor
        for fd in fds {
            // Only look at socket FDs
            if !matches!(fd.proc_fdtype.into(), ProcFDType::Socket) {
                continue;
            }

            // Get socket info
            let socket_info = match pidfdinfo::<SocketFDInfo>(pid, fd.proc_fd) {
                Ok(info) => info,
                Err(_) => continue,
            };

            // Check if this is a TCP socket
            let kind: SocketInfoKind = socket_info.psi.soi_kind.into();
            if !matches!(kind, SocketInfoKind::Tcp) {
                continue;
            }

            // Get the local port from the TCP socket info
            // Safety: We've verified this is a TCP socket via soi_kind
            let local_port = unsafe { socket_info.psi.soi_proto.pri_tcp.tcpsi_ini.insi_lport };

            if local_port == client_port as i32 {
                return build_process_info_macos(pid);
            }
        }
    }

    ProcessInfo::default()
}

#[cfg(target_os = "macos")]
fn build_process_info_macos(pid: i32) -> ProcessInfo {
    use libproc::libproc::proc_pid::pidpath;

    let path = pidpath(pid).ok().map(PathBuf::from);

    let name = path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string());

    let bundle_id = get_macos_bundle_id(&path);

    ProcessInfo {
        pid: Some(pid),
        name,
        path,
        bundle_id,
    }
}

/// On macOS, try to extract the bundle identifier from an app bundle.
/// For paths like `/Applications/Safari.app/Contents/MacOS/Safari`,
/// we read the Info.plist to get the CFBundleIdentifier.
#[cfg(target_os = "macos")]
fn get_macos_bundle_id(path: &Option<PathBuf>) -> Option<String> {
    let path = path.as_ref()?;
    let path_str = path.to_string_lossy();

    // Check if this is inside an .app bundle
    if let Some(app_idx) = path_str.find(".app/") {
        let app_path = &path_str[..app_idx + 4]; // Include ".app"
        let plist_path = format!("{}/Contents/Info.plist", app_path);

        // Try to read the bundle identifier from the plist
        if let Ok(contents) = std::fs::read_to_string(&plist_path) {
            // Simple XML parsing for CFBundleIdentifier
            if let Some(start) = contents.find("<key>CFBundleIdentifier</key>") {
                let after_key = &contents[start..];
                if let Some(string_start) = after_key.find("<string>") {
                    let value_start = string_start + 8;
                    if let Some(string_end) = after_key[value_start..].find("</string>") {
                        let bundle_id = &after_key[value_start..value_start + string_end];
                        return Some(bundle_id.to_string());
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn identify_process_linux(client_port: u16) -> ProcessInfo {
    // On Linux, we can read /proc/net/tcp to find which process owns the socket
    // Format: local_address:port remote_address:port ... inode
    // Then we search /proc/*/fd/* for the matching socket inode

    use std::fs;

    // Read /proc/net/tcp
    let tcp_content = match fs::read_to_string("/proc/net/tcp") {
        Ok(content) => content,
        Err(_) => return ProcessInfo::default(),
    };

    // Also check /proc/net/tcp6 for IPv6
    let tcp6_content = fs::read_to_string("/proc/net/tcp6").unwrap_or_default();

    let port_hex = format!("{:04X}", client_port);
    let mut target_inode: Option<u64> = None;

    // Search for the port in tcp tables
    for line in tcp_content.lines().chain(tcp6_content.lines()).skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 10 {
            continue;
        }

        // local_address is parts[1], format is "IP:PORT"
        let local_addr = parts[1];
        if let Some(port_part) = local_addr.split(':').last() {
            if port_part == port_hex {
                // Found it! Get the inode (parts[9])
                if let Ok(inode) = parts[9].parse::<u64>() {
                    target_inode = Some(inode);
                    break;
                }
            }
        }
    }

    let inode = match target_inode {
        Some(i) => i,
        None => return ProcessInfo::default(),
    };

    // Now find which process has this socket inode
    let proc_dir = match fs::read_dir("/proc") {
        Ok(dir) => dir,
        Err(_) => return ProcessInfo::default(),
    };

    for entry in proc_dir.flatten() {
        let pid_str = entry.file_name();
        let pid_str = pid_str.to_string_lossy();

        // Only look at numeric directories (PIDs)
        let pid: i32 = match pid_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Check file descriptors
        let fd_path = format!("/proc/{}/fd", pid);
        let fd_dir = match fs::read_dir(&fd_path) {
            Ok(dir) => dir,
            Err(_) => continue,
        };

        for fd_entry in fd_dir.flatten() {
            if let Ok(link) = fs::read_link(fd_entry.path()) {
                let link_str = link.to_string_lossy();
                if link_str.contains(&format!("socket:[{}]", inode)) {
                    return build_process_info_linux(pid);
                }
            }
        }
    }

    ProcessInfo::default()
}

#[cfg(target_os = "linux")]
fn build_process_info_linux(pid: i32) -> ProcessInfo {
    use std::fs;

    let exe_path = format!("/proc/{}/exe", pid);
    let path = fs::read_link(&exe_path).ok();

    let name = path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string());

    ProcessInfo {
        pid: Some(pid),
        name,
        path,
        bundle_id: None, // Linux doesn't have bundle IDs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_process_info_display_name() {
        let info = ProcessInfo {
            pid: Some(1234),
            name: Some("curl".to_string()),
            path: Some(PathBuf::from("/usr/bin/curl")),
            bundle_id: None,
        };
        assert_eq!(info.display_name(), "curl");

        let info_with_bundle = ProcessInfo {
            pid: Some(1234),
            name: Some("Safari".to_string()),
            path: Some(PathBuf::from(
                "/Applications/Safari.app/Contents/MacOS/Safari",
            )),
            bundle_id: Some("com.apple.Safari".to_string()),
        };
        assert_eq!(info_with_bundle.display_name(), "com.apple.Safari");

        let empty_info = ProcessInfo::default();
        assert_eq!(empty_info.display_name(), "");
    }

    #[test]
    fn test_is_identified() {
        let identified = ProcessInfo {
            pid: Some(1234),
            name: Some("test".to_string()),
            path: None,
            bundle_id: None,
        };
        assert!(identified.is_identified());

        let not_identified = ProcessInfo::default();
        assert!(!not_identified.is_identified());
    }

    #[test]
    fn test_non_loopback_returns_empty() {
        // Non-loopback addresses should return empty ProcessInfo
        let external_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 12345);
        let info = identify_process(&external_addr);
        assert!(!info.is_identified());
    }

    #[test]
    fn test_display_name_from_path() {
        let info = ProcessInfo {
            pid: Some(1234),
            name: None,
            path: Some(PathBuf::from("/usr/local/bin/my-program")),
            bundle_id: None,
        };
        assert_eq!(info.display_name(), "my-program");
    }
}
