#![allow(unreachable_code, unused_imports)]

use std::io::{Read, Write};
use teleport_coding_challenge::config;
use std::time::Duration;
use simple_logger::SimpleLogger;

use argparse::{ArgumentParser, StoreTrue, Store};

use log::{warn, info, error};

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    SimpleLogger::new().with_level(log::LevelFilter::Debug).init().unwrap();

    let mut id          = "".to_string();
    let mut port : u16  = 8443;
    let mut other_certs = false;

    {
        let mut ap = ArgumentParser::new();
        ap.set_description("TLS 1.3 Client");
        ap.refer(&mut id).add_option(&["--id"], Store, "Client ID that refers to the cert beinf used. E.G. --id 'first'");
        ap.refer(&mut port).add_option(&["--port"], Store, "The port that client will connect to on localhost. default: 8443");
        ap.refer(&mut other_certs).add_option(&["--other"], StoreTrue, "This flag changes the ca that has signed the server cert -- to test authentication with different CAs. NOTE: there is onlt first client with alternate ca.");
        ap.parse_args_or_exit();
    }

    info!("Init Client!");
    
    let client_config = config::create_client_tls_config(other_certs, &id)?;

    let mut stream = std::net::TcpStream::connect(format!("127.0.0.1:{port}"))?;

    info!("Connected to 127.0.0.1:8443");

    stream.set_read_timeout(Some(Duration::from_millis(1)))?;
    stream.set_write_timeout(Some(Duration::from_millis(1)))?;
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;

    let mut tls_conn = rustls::ClientConnection::new(client_config, "localhost".try_into()?)?;

    
    loop
    {
        
        if tls_conn.is_handshaking()
        {
            let mut tls_stream = rustls::Stream::new(&mut tls_conn, &mut stream);
            let mut buf : [u8; 1024] = [0; 1024];

            match tls_stream.read(&mut buf)
            {
                Ok(n) =>
                {
                    if n == 0
                    {
                        return Err("Connection closed".into());
                    }
                    info!("recv: {:#?}", buf);
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    // wait for next poll
                },
                Err(e) =>
                {
                    return Err(e.into());
                }
            }
        }
        else
        {
            break;
        }
    }

    std::thread::sleep(Duration::from_millis(1000));

    let mut tls_stream = rustls::Stream::new(&mut tls_conn, &mut stream);
    
    let mut cnt : usize = 0;

    loop
    {
        info!("sending: HELLO_{cnt}");

        match tls_stream.write_all(format!("HELLO_{cnt}").as_bytes())
        {
            Ok(()) =>
            {
                // Great
            },
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                // wait for next poll
            },
            Err(e) =>
            {
                return Err(e.into());
            }
        }

        cnt += 1;

        for _i in 0..5
        {
            let mut buf : [u8; 1024] = [0; 1024];

            match tls_stream.read(&mut buf)
            {
                Ok(n) =>
                {
                    if n == 0
                    {
                        return Err("Connection closed".into());
                    }
                    info!("recv: {:?}", String::from_utf8(buf[0..n].to_vec())?);
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    // wait for next poll
                },
                Err(e) =>
                {
                    return Err(e.into());
                }
            }

            std::thread::sleep(Duration::from_millis(10));
        }

        std::thread::sleep(Duration::from_millis(1000));
    }

    Ok(())
}

