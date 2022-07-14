#![allow(unreachable_code, unused_imports)]

use std::io::{Read, Write};
use teleport_coding_challenge::config;
use std::time::Duration;
use simple_logger::SimpleLogger;

use log::{warn, info, error};

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    SimpleLogger::new().with_level(log::LevelFilter::Debug).init().unwrap();

    info!("Init Client!");
    
    let client_config = config::create_client_tls_config()?;

    let mut stream = std::net::TcpStream::connect("127.0.0.1:8443")?;

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

        for i in 0..5
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

