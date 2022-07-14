use std::io::{Read, Write};
use teleport_coding_challenge::config;

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    let client_config = config::create_client_tls_config()?;

    let mut stream = std::net::TcpStream::connect("127.0.0.1:8443")?;

    let mut tls_conn = rustls::ClientConnection::new(client_config, "localhost".try_into()?)?;

    let mut tls_stream = rustls::Stream::new(&mut tls_conn, &mut stream);

    for i in 0..10
    {
        tls_stream.write_all(format!("HELLO_{i}").as_bytes());
    }

    loop
    {
        let mut buf : [u8; 1024] = [0; 1024];

        match tls_stream.read(&mut buf)
        {
            Ok(n) =>
            {
                println!("{:#?}", buf);
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

    Ok(())
}

