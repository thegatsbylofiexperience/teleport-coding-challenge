use crate::{ LoadBalancer,  client::Client, server::ServerGroup, server::HealthChecker };
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

fn load_private_key(filename: &str) -> Result<rustls::PrivateKey, Box<dyn std::error::Error>>
{
    let keyfile = std::fs::File::open(filename).expect("cannot open private key file");
    let mut reader = BufReader::new(keyfile);

    loop
    {
        match rustls_pemfile::read_one(&mut reader).expect("cannot parse private key .pem file")
        {
            Some(rustls_pemfile::Item::RSAKey(key)) => return Ok(rustls::PrivateKey(key)),
            Some(rustls_pemfile::Item::PKCS8Key(key)) => return Ok(rustls::PrivateKey(key)),
            Some(rustls_pemfile::Item::ECKey(key)) => return Ok(rustls::PrivateKey(key)),
            None => break,
            _ => {}
        }
    }

    return Err(format!("no keys found in {:?} (encrypted keys not supported)", filename).into());
}

pub fn create_server_tls_config(other_certs: bool) -> Result<Arc<rustls::ServerConfig>, Box<dyn std::error::Error>>
{
    let roots = if !other_certs
                {
                    load_certs("certs/cert/ec-cacert.pem".into())?
                }
                else
                {
                    load_certs("other_certs/cert/ec-cacert.pem".into())?
                };

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

    let certs = if ! other_certs
                {
                    load_certs("certs/server.pem")?
                }
                else
                {
                    load_certs("other_certs/server.pem")?
                };

    let privkey = if ! other_certs
                  {
                       load_private_key("certs/server.key")?
                  }
                  else
                  {
                       load_private_key("other_certs/server.key")?
                  };

    let ocsp : Vec<u8> = vec![];

    let mut config = rustls::ServerConfig::builder()
        .with_cipher_suites(&suites)
        .with_safe_default_kx_groups()
        .with_protocol_versions(versions.as_slice())?
        .with_client_cert_verifier(client_auth)
        .with_single_cert_with_ocsp_and_sct(certs, privkey, ocsp, vec![])?;

    config.key_log = Arc::new(rustls::KeyLogFile::new());

    Ok(Arc::new(config))
}

pub fn create_client_tls_config(other_certs: bool, id: &String) -> Result<Arc<rustls::ClientConfig>, Box<dyn std::error::Error>>
{
    let mut root_store = RootCertStore::empty(); 

    let certfile = if !other_certs
                   {
                       std::fs::File::open(&"certs/cert/ec-cacert.pem")?
                   }
                   else
                   {
                       std::fs::File::open(&"other_certs/cert/ec-cacert.pem")?
                   };

    let mut reader = BufReader::new(certfile);
    root_store.add_parsable_certificates(&rustls_pemfile::certs(&mut reader)?);

    let suites = vec![ 
						 rustls::cipher_suite::TLS13_AES_128_GCM_SHA256, 
						 rustls::cipher_suite::TLS13_AES_256_GCM_SHA384, 
					 ];

    let versions : Vec<&'static rustls::SupportedProtocolVersion> = vec![&rustls::version::TLS13];

    let config = rustls::ClientConfig::builder()
                 .with_cipher_suites(&suites)
                 .with_safe_default_kx_groups()
                 .with_protocol_versions(&versions)?
                 .with_root_certificates(root_store);

    let certs = if !other_certs
                {
                    load_certs(&format!("certs/{id}.crt"))?
                }
                else
                {
                    load_certs(&format!("other_certs/{id}.crt"))?
                };

    let key = if !other_certs
              {
                  load_private_key(&format!("certs/{id}.key"))?
              }
              else
              {
                  load_private_key(&format!("other_certs/{id}.key"))?
              };

    let mut conf = config.with_single_cert(certs, key)?;

    conf.key_log = Arc::new(rustls::KeyLogFile::new());

    Ok(Arc::new(conf))
}

pub fn load_configuration(port: u16, other_certs: bool) -> Result<LoadBalancer, Box<dyn std::error::Error>>
{
	let tls_conf = create_server_tls_config(other_certs)?;

    let mut lb = LoadBalancer::new(tls_conf, port)?;

    lb.clients.insert("first@first.com".into(), Client::new("first@first.com".into(), 0));
    lb.clients.insert("second@second.com".into(), Client::new("second@second.com".into(), 1));
    lb.clients.insert("third@third.com".into(), Client::new("third@third.com".into(), 2));
    lb.clients.insert("fourth@fourth.com".into(), Client::new("fourth@fourth.com".into(), 3));

    let mut sg0 = ServerGroup::new(0);

    sg0.add_server(0, "127.0.0.1:2500".into());
    sg0.add_server(1, "127.0.0.1:2501".into());
    sg0.add_server(2, "127.0.0.1:2502".into());

    lb.server_groups.insert(0, sg0);
    
    let mut sg1 = ServerGroup::new(1);

    sg1.add_server(3, "127.0.0.1:2503".into());
    sg1.add_server(4, "127.0.0.1:2504".into());
    sg1.add_server(5, "127.0.0.1:2505".into());

    lb.server_groups.insert(1, sg1);

    return Ok(lb);
}


