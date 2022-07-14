use crate::{ LoadBalancer,  Client, ServerGroup, HealthChecker };
use std::sync::Arc;
use rustls::server::AllowAnyAuthenticatedClient;
use rustls::{self, RootCertStore};
use std::io::{BufReader};


fn load_certs(filename: &str) -> Result<Vec<rustls::Certificate>, Box<dyn std::error::Error>>
{
    let certfile = std::fs::File::open(filename)?;

    let mut reader = BufReader::new(certfile);

    Ok(rustls_pemfile::certs(&mut reader)?
        .iter()
        .map(|v| rustls::Certificate(v.clone()))
        .collect())
}

fn load_private_key(filename: &str) -> rustls::PrivateKey {
    let keyfile = std::fs::File::open(filename).expect("cannot open private key file");
    let mut reader = BufReader::new(keyfile);

    loop {
        match rustls_pemfile::read_one(&mut reader).expect("cannot parse private key .pem file") {
            Some(rustls_pemfile::Item::RSAKey(key)) => return rustls::PrivateKey(key),
            Some(rustls_pemfile::Item::PKCS8Key(key)) => return rustls::PrivateKey(key),
            Some(rustls_pemfile::Item::ECKey(key)) => return rustls::PrivateKey(key),
            None => break,
            _ => {}
        }
    }

    panic!(
        "no keys found in {:?} (encrypted keys not supported)",
        filename
    );
}

fn create_tls_config() -> Result<Arc<rustls::ServerConfig>, Box<dyn std::error::Error>>
{
    let roots = load_certs("ca.cert".into())?;
    let mut client_auth_roots = RootCertStore::empty();
    for root in roots
    {
        client_auth_roots.add(&root).unwrap();
    }
    
	let client_auth = AllowAnyAuthenticatedClient::new(client_auth_roots);

    let suites = vec![ 
						 rustls::cipher_suite::TLS13_AES_128_GCM_SHA256, 
						 rustls::cipher_suite::TLS13_AES_256_GCM_SHA384, 
					 ];

    let versions : Vec<&'static rustls::SupportedProtocolVersion> = vec![&rustls::version::TLS13];

    let certs = load_certs("server.cert")?;

    let privkey = load_private_key("server.key");

    let ocsp : Vec<u8> = vec![];

    let mut config = rustls::ServerConfig::builder()
        .with_cipher_suites(&suites)
        .with_safe_default_kx_groups()
        .with_protocol_versions(versions.as_slice())
        .expect("inconsistent cipher-suites/versions specified")
        .with_client_cert_verifier(client_auth)
        .with_single_cert_with_ocsp_and_sct(certs, privkey, ocsp, vec![])
        .expect("bad certificates/private key");

    config.key_log = Arc::new(rustls::KeyLogFile::new());

    Ok(Arc::new(config))
}

pub fn load_configuration() -> Result<LoadBalancer, Box<dyn std::error::Error>>
{
	let tls_conf = create_tls_config()?;

    let mut lb = LoadBalancer::new(tls_conf)?;

    lb.clients.insert("first@first.com".into(), Client::new("first@first.com".into(), 0));
    lb.clients.insert("second@second.com".into(), Client::new("second@second.com".into(), 1));
    lb.clients.insert("third@third.com".into(), Client::new("third@third.com".into(), 2));
    lb.clients.insert("fourth@fourth.com".into(), Client::new("fourth@fourth.com".into(), 3));

    let mut sg0 = ServerGroup::new(0);

    sg0.server_addrs.insert(0, "127.0.0.1:2500".into());
    sg0.server_health.insert(0, HealthChecker::new(0, "127.0.0.1:2500".into()));
    sg0.server_addrs.insert(1, "127.0.0.1:2501".into());
    sg0.server_health.insert(1, HealthChecker::new(1, "127.0.0.1:2501".into()));
    sg0.server_addrs.insert(2, "127.0.0.1:2502".into());
    sg0.server_health.insert(2, HealthChecker::new(2, "127.0.0.1:2502".into()));

    lb.server_groups.insert(0, sg0);
    
    let mut sg1 = ServerGroup::new(1);

    sg1.server_addrs.insert(3, "127.0.0.1:2500".into());
    sg1.server_health.insert(3, HealthChecker::new(3, "127.0.0.1:2503".into()));
    sg1.server_addrs.insert(4, "127.0.0.1:2501".into());
    sg1.server_health.insert(4, HealthChecker::new(4, "127.0.0.1:2504".into()));
    sg1.server_addrs.insert(5, "127.0.0.1:2502".into());
    sg1.server_health.insert(5, HealthChecker::new(5, "127.0.0.1:2505".into()));

    lb.server_groups.insert(0, sg1);

    return Ok(lb);
}


