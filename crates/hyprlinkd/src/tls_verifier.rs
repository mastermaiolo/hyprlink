//! Verificador de certificado de cliente que espelha o comportamento do app
//! Android: nenhuma validação de CA (certificados são autoassinados dos dois
//! lados), mas a prova de posse de chave continua sendo verificada — quem
//! autentica de verdade é o pin de fingerprint pós-handshake (ver `pairing.rs`),
//! não este verificador.

use std::sync::Arc;

use rustls::client::danger::HandshakeSignatureValid;
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};

#[derive(Debug)]
pub struct AcceptAnyClientCert {
    supported_algs: WebPkiSupportedAlgorithms,
}

impl AcceptAnyClientCert {
    pub fn new(provider: &CryptoProvider) -> Arc<Self> {
        Arc::new(Self {
            supported_algs: provider.signature_verification_algorithms,
        })
    }
}

impl ClientCertVerifier for AcceptAnyClientCert {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        // Sem CA: qualquer certificado passa aqui. A autenticação real é o
        // pin de fingerprint feito depois do handshake, em server.rs.
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.supported_algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.supported_algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported_algs.supported_schemes()
    }
}
