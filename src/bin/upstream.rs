#![allow(unreachable_code, unused_imports)]

use std::time::Duration;
use std::io::Read;
use std::io::Write;
use log::{warn, info, error};
use simple_logger::SimpleLogger;
use argparse::{ArgumentParser, StoreTrue, Store};


// Upstream Server
// Just a straight TcpListener which listens for streams.
// No encryption! 
// Does two things!
// 1. listens for data and echos it back
// 2. listens for the string "PING" and returns PONG 
//
// For simplicity it is expected that all messages will fit within the buffer of 1024 bytes. This would not work in practise but for demonstration/testing this will suffice.
fn main() -> Result<(), Box<dyn std::error::Error>>
{
    SimpleLogger::new().with_level(log::LevelFilter::Debug).init().unwrap();
    
    let mut port : u16 = 2500;

    {
        let mut ap = ArgumentParser::new();
        ap.set_description("TLS 1.3 Upstream Server");
        ap.refer(&mut port).add_option(&["--port"], Store, "The port that the upstream will listen to on localhost. default: 2500");
        ap.parse_args_or_exit();
    }

    info!("INIT Upstream!");

    let addr = format!("127.0.0.1:{port}");

    let listener = std::net::TcpListener::bind(addr)?;

    info!("listening on 127.0.0.1:{port}");

    listener.set_nonblocking(true)?;

    let mut streams : Vec<std::net::TcpStream> = vec![];

    loop
    {
        for stream_res in listener.incoming()
        {
			match stream_res
			{
				Ok(stream) =>
				{
                    info!("Connection!");

                    stream.set_read_timeout(Some(Duration::from_millis(1)))?;
                    stream.set_write_timeout(Some(Duration::from_millis(1)))?;
                    stream.set_nonblocking(true)?;
                    stream.set_nodelay(true)?;

					// Handle new stream
                    streams.push(stream);
				},
    			Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    			{
					// Do nothing we will 
					break;
    			},
    			Err(e) =>
    			{
                    error!("{e}");
				}
			}
        }

        let mut to_remove : Vec<usize> = vec![];

        for (i, stream) in streams.iter_mut().enumerate()
        {
            let mut buf : [u8; 1024] = [0; 1024];
            match stream.read(&mut buf)
            {
                Ok(n) =>
                {
                    if n == 4 && buf[0..n] == *"PING".as_bytes()
                    {
                        info!("Received Ping");
                        // Send back "PONG"
                        match stream.write_all("PONG".as_bytes())
                        {
                            Ok(()) =>
                            {
                                // Great!
                                info!("Sent Pong");
                            },
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                            {
                                // wait for next poll
                            },
                            Err(e) =>
                            {
                                error!("{e}");
                                to_remove.push(i);
                            }
                        }
                    }
                    else
                    {
                        // write buffer back
                        match stream.write_all(&buf[0..n])
                        {
                            Ok(()) =>
                            {
                                // Great!
                            },
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                            {
                                // wait for next poll
                            },
                            Err(e) =>
                            {
                                error!("{e}");
                                to_remove.push(i);
                            }
                        }
                    }
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    // wait for next poll
                },
                Err(e) =>
                {
                    error!("{e}");
                    to_remove.push(i);
                }
            }
        }

        for i in to_remove.iter().rev()
        {
            streams.remove(*i);
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    Ok(())
}
