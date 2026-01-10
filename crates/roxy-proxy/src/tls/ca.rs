//! Certificate Authority for TLS Interception
//!
//! This module handles generating and managing a root CA certificate
//! that can sign server certificates on demand for TLS interception.

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, DnValue,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info};

/// Errors that can occur during CA operations
#[derive(Debug, Error)]
pub enum CertificateAuthorityError {
    #[error("Failed to generate key pair: {0}")]
    KeyGenerationError(String),

    #[error("Failed to generate certificate: {0}")]
    CertificateGenerationError(String),

    #[error("Failed to read file: {0}")]
    FileReadError(#[from] std::io::Error),

    #[error("Failed to parse PEM: {0}")]
    PemParseError(String),

    #[error("Failed to parse private key: {0}")]
    PrivateKeyParseError(String),

    #[error("Rustls error: {0}")]
    RustlsError(String),
}

/// Certificate Authority that can sign certificates for TLS interception
pub struct CertificateAuthority {
    /// The CA certificate in PEM format
    cert_pem: String,
    /// The CA certificate in DER format (for signing)
    cert_der: Vec<u8>,
    /// The CA key pair
    key_pair: Arc<KeyPair>,
}

impl CertificateAuthority {
    /// Create a new CA with a freshly generated key pair and certificate
    pub fn new() -> Result<Self, CertificateAuthorityError> {
        let key_pair = KeyPair::generate()
            .map_err(|e| CertificateAuthorityError::KeyGenerationError(e.to_string()))?;

        let mut params = CertificateParams::default();

        // Set up distinguished name
        let mut dn = DistinguishedName::new();
        dn.push(
            DnType::CommonName,
            DnValue::Utf8String("Roxy Proxy CA".to_string()),
        );
        dn.push(
            DnType::OrganizationName,
            DnValue::Utf8String("Roxy".to_string()),
        );
        dn.push(
            DnType::OrganizationalUnitName,
            DnValue::Utf8String("Development".to_string()),
        );
        params.distinguished_name = dn;

        // CA specific settings
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];

        // Valid for 10 years
        params.not_before = time::OffsetDateTime::now_utc();
        params.not_after =
            time::OffsetDateTime::now_utc() + Duration::from_secs(10 * 365 * 24 * 60 * 60);

        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| CertificateAuthorityError::CertificateGenerationError(e.to_string()))?;

        let cert_pem = cert.pem();
        let cert_der = cert.der().to_vec();

        info!("Generated new CA certificate");

        Ok(Self {
            cert_pem,
            cert_der,
            key_pair: Arc::new(key_pair),
        })
    }

    /// Load a CA from PEM files, or create a new one if they don't exist
    pub fn load_or_create(
        cert_path: &str,
        key_path: &str,
    ) -> Result<Self, CertificateAuthorityError> {
        let cert_path = Path::new(cert_path);
        let key_path = Path::new(key_path);

        if cert_path.exists() && key_path.exists() {
            Self::load(cert_path, key_path)
        } else {
            let ca = Self::new()?;
            ca.save(cert_path, key_path)?;
            Ok(ca)
        }
    }

    /// Load a CA from PEM files
    pub fn load(cert_path: &Path, key_path: &Path) -> Result<Self, CertificateAuthorityError> {
        let cert_pem = fs::read_to_string(cert_path)?;
        let key_pem = fs::read_to_string(key_path)?;

        let key_pair = KeyPair::from_pem(&key_pem)
            .map_err(|e| CertificateAuthorityError::PrivateKeyParseError(e.to_string()))?;

        // Parse PEM to get DER bytes
        let cert_der =
            pem_to_der(&cert_pem).map_err(|e| CertificateAuthorityError::PemParseError(e))?;

        info!(path = %cert_path.display(), "Loaded CA certificate from disk");

        Ok(Self {
            cert_pem,
            cert_der,
            key_pair: Arc::new(key_pair),
        })
    }

    /// Save the CA certificate and key to PEM files
    pub fn save(&self, cert_path: &Path, key_path: &Path) -> Result<(), CertificateAuthorityError> {
        // Create parent directories if needed
        if let Some(parent) = cert_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(cert_path, &self.cert_pem)?;
        fs::write(key_path, self.key_pair.serialize_pem())?;

        info!(
            cert = %cert_path.display(),
            key = %key_path.display(),
            "Saved CA certificate and key"
        );

        Ok(())
    }

    /// Generate a certificate for the given hostname, signed by this CA
    pub fn generate_cert(
        &self,
        hostname: &str,
    ) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), CertificateAuthorityError> {
        let key_pair = KeyPair::generate()
            .map_err(|e| CertificateAuthorityError::KeyGenerationError(e.to_string()))?;

        let mut params = CertificateParams::default();

        // Set up distinguished name
        let mut dn = DistinguishedName::new();
        dn.push(
            DnType::CommonName,
            DnValue::Utf8String(hostname.to_string()),
        );
        dn.push(
            DnType::OrganizationName,
            DnValue::Utf8String("Roxy".to_string()),
        );
        params.distinguished_name = dn;

        // Not a CA
        params.is_ca = IsCa::NoCa;

        // Server certificate key usage
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

        // Add the hostname as a SAN
        params.subject_alt_names = vec![SanType::DnsName(hostname.try_into().map_err(|e| {
            CertificateAuthorityError::CertificateGenerationError(format!(
                "Invalid hostname '{}': {}",
                hostname, e
            ))
        })?)];

        // Also add wildcard if this looks like a base domain
        if !hostname.starts_with("*.") && hostname.matches('.').count() >= 1 {
            // Try to add wildcard SAN, but don't fail if it doesn't work
            let wildcard = format!("*.{}", hostname);
            if let Ok(san) = wildcard.as_str().try_into() {
                params.subject_alt_names.push(SanType::DnsName(san));
            }
        }

        // Valid for 1 year
        params.not_before = time::OffsetDateTime::now_utc();
        params.not_after =
            time::OffsetDateTime::now_utc() + Duration::from_secs(365 * 24 * 60 * 60);

        // Create a signing certificate from our CA's DER and key
        let ca_cert_der = CertificateDer::from(self.cert_der.clone());
        let ca_params = CertificateParams::from_ca_cert_der(&ca_cert_der)
            .map_err(|e| CertificateAuthorityError::PemParseError(e.to_string()))?;
        let ca_cert = ca_params
            .self_signed(&self.key_pair)
            .map_err(|e| CertificateAuthorityError::CertificateGenerationError(e.to_string()))?;

        // Sign with the CA
        let cert = params
            .signed_by(&key_pair, &ca_cert, &self.key_pair)
            .map_err(|e| CertificateAuthorityError::CertificateGenerationError(e.to_string()))?;

        debug!(hostname = %hostname, "Generated certificate for host");

        // Convert to rustls types
        let cert_der = CertificateDer::from(cert.der().to_vec());
        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));

        Ok((cert_der, key_der))
    }

    /// Get the CA certificate in PEM format
    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    /// Get the CA certificate in DER format
    pub fn cert_der(&self) -> CertificateDer<'static> {
        CertificateDer::from(self.cert_der.clone())
    }
}

/// Parse PEM certificate to DER bytes
fn pem_to_der(pem: &str) -> Result<Vec<u8>, String> {
    // Simple PEM parser - find the base64 content between BEGIN and END markers
    let begin_marker = "-----BEGIN CERTIFICATE-----";
    let end_marker = "-----END CERTIFICATE-----";

    let start = pem
        .find(begin_marker)
        .ok_or_else(|| "Missing BEGIN CERTIFICATE marker".to_string())?
        + begin_marker.len();
    let end = pem
        .find(end_marker)
        .ok_or_else(|| "Missing END CERTIFICATE marker".to_string())?;

    let base64_content: String = pem[start..end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &base64_content)
        .map_err(|e| format!("Failed to decode base64: {}", e))
}

impl Default for CertificateAuthority {
    fn default() -> Self {
        Self::new().expect("Failed to create default CA")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_ca_generation() {
        let ca = CertificateAuthority::new().unwrap();
        assert!(!ca.cert_pem().is_empty());
        assert!(ca.cert_pem().contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn test_ca_save_and_load() {
        let dir = tempdir().unwrap();
        let cert_path = dir.path().join("ca.crt");
        let key_path = dir.path().join("ca.key");

        // Create and save
        let ca1 = CertificateAuthority::new().unwrap();
        ca1.save(&cert_path, &key_path).unwrap();

        // Load
        let ca2 = CertificateAuthority::load(&cert_path, &key_path).unwrap();

        // The PEM should match
        assert_eq!(ca1.cert_pem(), ca2.cert_pem());
        // The DER should also match
        assert_eq!(ca1.cert_der.len(), ca2.cert_der.len());
    }

    #[test]
    fn test_load_or_create_creates() {
        let dir = tempdir().unwrap();
        let cert_path = dir.path().join("ca.crt").to_string_lossy().to_string();
        let key_path = dir.path().join("ca.key").to_string_lossy().to_string();

        // Should create new
        let ca = CertificateAuthority::load_or_create(&cert_path, &key_path).unwrap();
        assert!(!ca.cert_pem().is_empty());

        // Files should exist
        assert!(Path::new(&cert_path).exists());
        assert!(Path::new(&key_path).exists());
    }

    #[test]
    fn test_generate_server_cert() {
        let ca = CertificateAuthority::new().unwrap();
        let (cert, key) = ca.generate_cert("example.com").unwrap();

        assert!(!cert.as_ref().is_empty());
        match &key {
            PrivateKeyDer::Pkcs8(k) => assert!(!k.secret_pkcs8_der().is_empty()),
            _ => panic!("Expected PKCS8 key"),
        }
    }

    #[test]
    fn test_generate_cert_for_ip_like_host() {
        let ca = CertificateAuthority::new().unwrap();
        // This should work for any string that can be a DNS name
        let result = ca.generate_cert("api.example.com");
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_cert_for_subdomain() {
        let ca = CertificateAuthority::new().unwrap();
        let result = ca.generate_cert("sub.api.example.com");
        assert!(result.is_ok());
    }
}
