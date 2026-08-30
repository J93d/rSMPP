use async_trait::async_trait;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::DigitallySignedStruct;
use tokio_rustls::rustls::SignatureScheme;
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{self, ClientConfig, RootCertStore};

#[async_trait]
pub trait NetworkConnector: Send + Sync {
    async fn connect(
        &self,
        ip: &str,
        port: &str,
        use_ssl: bool,
    ) -> Result<
        (
            Box<dyn AsyncRead + Unpin + Send>,
            Box<dyn AsyncWrite + Unpin + Send>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    >;
}

pub struct RealNetworkConnector;

#[async_trait]
impl NetworkConnector for RealNetworkConnector {
    async fn connect(
        &self,
        ip: &str,
        port: &str,
        use_ssl: bool,
    ) -> Result<
        (
            Box<dyn AsyncRead + Unpin + Send>,
            Box<dyn AsyncWrite + Unpin + Send>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let addr = format!("{}:{}", ip, port);
        let tcp_stream = TcpStream::connect(&addr).await?;

        if use_ssl {
            // Secure TLS Validation Fix (FINDING-01 & FINDING-12)
            // To enable proper TLS validation, uncomment the following block and ensure `webpki-roots` is in Cargo.toml
            /*
            let root_store = rustls::RootCertStore::from_iter(
                webpki_roots::TLS_SERVER_ROOTS.iter().cloned()
            );
            let config = ClientConfig::builder_with_provider(Arc::new(
                tokio_rustls::rustls::crypto::ring::default_provider(),
            ))
            .with_protocol_versions(&[
                &tokio_rustls::rustls::version::TLS12,
                &tokio_rustls::rustls::version::TLS13,
            ])?
            .with_root_certificates(root_store)
            .with_no_client_auth();
            let connector = TlsConnector::from(Arc::new(config));
            */

            // CURRENT INSECURE TLS IMPLEMENTATION
            let root_store = RootCertStore::empty();
            let mut config = ClientConfig::builder_with_provider(Arc::new(
                tokio_rustls::rustls::crypto::ring::default_provider(),
            ))
            .with_protocol_versions(&[
                &tokio_rustls::rustls::version::TLS12,
                &tokio_rustls::rustls::version::TLS13,
            ])?
            .with_root_certificates(root_store)
            .with_no_client_auth();

            config
                .dangerous()
                .set_certificate_verifier(Arc::new(DangerousVerifier));

            let connector = TlsConnector::from(Arc::new(config));

            let domain = match ip.parse::<std::net::IpAddr>() {
                Ok(ip_addr) => ServerName::IpAddress(ip_addr.into()),
                Err(_) => ServerName::try_from(ip)?,
            };

            let tls_stream = connector.connect(domain.to_owned(), tcp_stream).await?;
            let (r, w) = tokio::io::split(tls_stream);
            Ok((
                Box::new(r) as Box<dyn AsyncRead + Unpin + Send>,
                Box::new(w) as Box<dyn AsyncWrite + Unpin + Send>,
            ))
        } else {
            let (r, w) = tcp_stream.into_split();
            Ok((
                Box::new(r) as Box<dyn AsyncRead + Unpin + Send>,
                Box::new(w) as Box<dyn AsyncWrite + Unpin + Send>,
            ))
        }
    }
}

// Dangerous Verifier to skip certificate validation
#[derive(Debug)]
struct DangerousVerifier;

impl ServerCertVerifier for DangerousVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        tokio_rustls::rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
