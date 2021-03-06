#![allow(non_camel_case_types, unused_variables, dead_code, unused_assignments, unused_imports)]

use std::net::{TcpListener, TcpStream};
use std::collections::*;

use std::io::{Write, Read};

use std::time::Duration;

use std::sync::Arc;
use rustls;

use x509_parser::prelude::*;

use log::{trace, debug, info, warn, error};

pub struct Client
{
    email                : String,
    connections          : Vec<Connection>,
    cxn_time             : i64,
    cxn_cnt              : usize,
    allowed_server_group : u32,
}

impl Client
{
    pub fn new(email: String, allowed_server_group: u32) -> Self
    {
        Self { email, connections: vec![], cxn_time: i64::MIN, cxn_cnt: 0,  allowed_server_group }
    }

    pub fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        // poll connections
		for cxn in self.connections.iter_mut()
        {
            cxn.poll()?;
        }

        Ok(())
    }

    pub fn get_server_group(&self) -> u32
    {
        self.allowed_server_group
    }

    pub fn add_connection(&mut self, cxn: Connection)
    {
        // convert current ts to i64 and divide 30 to give current 30s period
        let now : i64 = chrono::Utc::now().timestamp() / 30;

        let mut ok = true;

        if now == self.cxn_time
        {
            if self.cxn_cnt >= 10
            {
                ok = false;
                error!("Client rate limit hit for {}", self.email);
            }
        }
        // cxn_time does not match start a new 30s period
        else
        {
            self.cxn_time = now;
            self.cxn_cnt  = 0;
        }

        if ok
        {
            self.connections.push(cxn);
            self.cxn_cnt += 1;
        }
    }

    pub fn cleanup_connections(&mut self) -> Vec<Connection>
    {
        let mut to_remove : Vec<usize> = vec![];

        for (i, cxn) in self.connections.iter().enumerate()
        {
            match cxn.conn_state
            {
                ConnState::UP_DISCONNECT    |
                ConnState::UP_TIMEOUT       | 
                ConnState::DOWN_DISCONNECT  |
                ConnState::DOWN_TIMEOUT     |
                ConnState::DOWN_ENC_ERR     =>
                {
                    to_remove.push(i);
                }
                _ => {}
            }
        }

        let mut out : Vec<Connection> = vec![];

        for i in to_remove.iter().rev()
        {
            out.push(self.connections.remove(*i));
        }

        out
    }
}

#[test]
fn test_client_cleanup()
{
    let addr: String = "127.0.0.1:25014".into();

    // Minimum code to get connections working
    // they are not needed apart from 
    let listener = std::net::TcpListener::bind(addr.clone()).unwrap();
    listener.set_nonblocking(true).unwrap();

    let mut cli = Client::new("".to_string(), 0);


	// TLS setup that is ot used other than for creation of Connection struct
	let config = crate::config::create_server_tls_config(false).unwrap();

    for i in 0..8
    {
        let down_stream = TcpStream::connect(addr.clone()).unwrap();
        let up_stream = TcpStream::connect(addr.clone()).unwrap();
		let tls_conn = rustls::ServerConnection::new(Arc::clone(&config)).unwrap();
        

        let mut cxn = Connection::new(down_stream, up_stream, tls_conn, 0, 0).unwrap();

        match i
        {
            0 => { cxn.conn_state = ConnState::OKAY; },
            1 => { cxn.conn_state = ConnState::OKAY; },
            2 => { cxn.conn_state = ConnState::OKAY; },
            3 => { cxn.conn_state = ConnState::UP_DISCONNECT; },
            4 => { cxn.conn_state = ConnState::UP_TIMEOUT; },
            5 => { cxn.conn_state = ConnState::DOWN_DISCONNECT; },
            6 => { cxn.conn_state = ConnState::DOWN_TIMEOUT; },
            7 => { cxn.conn_state = ConnState::DOWN_ENC_ERR; }
            _ => {},
        }

        cli.connections.push(cxn);
    }

    let bad_connections = cli.cleanup_connections();

    assert!(bad_connections.len() == 5);
    assert!(cli.connections.len() == 3);
}

#[test]
fn test_client_rate_limiter()
{
    let addr: String = "127.0.0.1:25015".into();

    // Minimum code to get connections working
    // they are not needed apart from 
    let listener = std::net::TcpListener::bind(addr.clone()).unwrap();
    listener.set_nonblocking(true).unwrap();

    let mut cli = Client::new("".to_string(), 0);


	// TLS setup that is ot used other than for creation of Connection struct
	let config = crate::config::create_server_tls_config(false).unwrap();

    for i in 0..20
    {
        let down_stream = TcpStream::connect(addr.clone()).unwrap();
        let up_stream = TcpStream::connect(addr.clone()).unwrap();
		let tls_conn = rustls::ServerConnection::new(Arc::clone(&config)).unwrap();
        

        let cxn = Connection::new(down_stream, up_stream, tls_conn, 0, 0).unwrap();

        cli.add_connection(cxn);
    }

	println!("{}", cli.connections.len());
    assert!(cli.connections.len() == 10);
}


#[derive(Clone, PartialEq, Eq)]
enum PartialConnState
{
    INIT,
    COMPLETED,
    ERROR,
}


//TODO: Not testing *YET* -- does nothing but act as a stub for encryption
pub struct PartialConnection
{
    down_stream         : std::net::TcpStream,
    tls_conn            : rustls::ServerConnection,
    state               : PartialConnState,
    email_address       : Option<String>,
    
}

impl PartialConnection
{
    pub fn new(down_stream: std::net::TcpStream, tls_conn: rustls::ServerConnection) -> Self
    {
        Self { down_stream, tls_conn, state: PartialConnState::INIT, email_address: None }
    }

    pub fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        let mut next_state = self.state.clone();

		match self.state
        {
            PartialConnState::INIT =>
            {
                // Handle authentication / authorisation
                if self.tls_conn.is_handshaking()
                {
                    let mut tls_stream = rustls::Stream::new(&mut self.tls_conn, &mut self.down_stream);

                    let mut buf : [u8; 2048] = [0; 2048];

                    match tls_stream.read(&mut buf)
                    {
                        Ok(n) =>
                        {
                            if n == 0
                            {
                                next_state = PartialConnState::ERROR;
                            }
                        },
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                        {
                            // wait for next poll
                        },
                        Err(_e) =>
                        {
                            next_state = PartialConnState::ERROR;
                        }
                    }
                }
                else
                {
                    // handshake done
                    // get cert
                    if let Some(certs) = self.tls_conn.peer_certificates()
                    {
                        match extract_email_from_cert(&certs[0].0)
                        {
                            Ok(email) =>
                            {
                                info!("Email found: {email}");

                                self.email_address = Some(email);
                                next_state = PartialConnState::COMPLETED;
                            },
                            Err(e) =>
                            {
                                next_state = PartialConnState::ERROR;
                                error!("Email address not found in client cert.");
                                error!("{e}");
                            }
                        }
                    }
                    else
                    {
                        next_state = PartialConnState::ERROR;
                        error!("Client certs not found.");
                    }
                }
            },
            _ =>
            {
                // Either completed or in error
                // Do nothing and wait for clean up or conversion
            }
        }

        self.state = next_state;

        Ok(())
    }

    pub fn is_completed(&self) -> bool
    {
        return self.state == PartialConnState::COMPLETED;
    }

    pub fn client_id(&self) -> Option<String>
    {
        self.email_address.clone()
    }
}

fn extract_email_from_cert(cert: &Vec<u8>) -> Result<String, Box<dyn std::error::Error>>
{
    let (rem, cert) = X509Certificate::from_der(&cert[..])?;
    for email in cert.subject().iter_email()
    {
        return Ok(email.as_str()?.to_string());
    }

	return Err("No email address found".into());
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ConnState
{
    OKAY,
    UP_DISCONNECT,
    UP_TIMEOUT,
    DOWN_DISCONNECT,
    DOWN_TIMEOUT,
    DOWN_ENC_ERR,
}

pub struct Connection
{
    down_stream         : std::net::TcpStream,
    up_stream           : std::net::TcpStream,
    tls_conn            : rustls::ServerConnection,
    conn_state          : ConnState,
    upstream_serv_group : u32,
    upstream_serv_id    : u32,
}

impl Connection
{
    pub fn new(down_stream: std::net::TcpStream, up_stream: std::net::TcpStream, tls_conn: rustls::ServerConnection, upstream_serv_group: u32, upstream_serv_id: u32) -> Result<Self, Box<dyn std::error::Error>>
    {
        Ok(Self { down_stream, up_stream, tls_conn, conn_state: ConnState::OKAY, upstream_serv_group, upstream_serv_id })
    }

    pub fn from_partial_connection(partial_cxn: PartialConnection, upstream_serv_group: u32, upstream_serv_id: u32, up_stream_addr: &String) -> Result<Self, Box<dyn std::error::Error>>
    {
        // TODO: Blocking call. Move to call with a timeout or wrap in a thread.
        let up_stream = std::net::TcpStream::connect_timeout(&up_stream_addr.parse()?, Duration::from_millis(100))?;
        up_stream.set_read_timeout(Some(Duration::from_millis(1)))?;
        up_stream.set_write_timeout(Some(Duration::from_millis(1)))?;
        up_stream.set_nonblocking(true)?;
        up_stream.set_nodelay(true)?;

        Self::new(partial_cxn.down_stream, up_stream, partial_cxn.tls_conn, upstream_serv_group, upstream_serv_id)
    }

    pub fn get_upstream_server_group(&self) -> u32
    {
        self.upstream_serv_group
    }

    pub fn get_upstream_server_id(&self) -> u32
    {
        self.upstream_serv_id
    }

    fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        let mut next_state = self.conn_state.clone();

        match self.conn_state
        {
            ConnState::OKAY =>
            {
                // luckily this is a stateless stream object
                // all state kept in tls_conn and down_stream
                let mut tls_stream = rustls::Stream::new(&mut self.tls_conn, &mut self.down_stream);

                let mut down_buf   : [u8; 2048] = [0; 2048];
                match tls_stream.read(&mut down_buf)
                {
                    Ok(n) =>
                    {
                        trace!("Received: {n} bytes");

                        if n == 0
                        {
                            next_state = ConnState::DOWN_DISCONNECT;
                            error!("Client DC");
                        }
                        else
                        {
                            // write buffer back
                            match self.up_stream.write_all(&down_buf[0..n])
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
                                    next_state = ConnState::UP_DISCONNECT;
                                    error!("UPSTREAM DC: {e}");
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
                        next_state = ConnState::DOWN_DISCONNECT;
                        error!("Client DC: {e}");
                    }
                }

                let mut up_buf   : [u8; 2048] = [0; 2048];
                match self.up_stream.read(&mut up_buf)
                {
                    Ok(n) =>
                    {
                        trace!("Sent: {n} bytes");

                        if n == 0
                        {
                            next_state = ConnState::UP_DISCONNECT;
                            error!("UPSTREAM DC");
                        }
                        else
                        {
                            // write buffer back
                            match tls_stream.write_all(&up_buf[0..n])
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
                                    next_state = ConnState::DOWN_DISCONNECT;
                                    error!("Client DC: {e}");
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
                        next_state = ConnState::UP_DISCONNECT;
                        error!("UPSTREAM DC: {e}");
                    }
                }
            },
            _ =>
            {
                // Connection is in error.
                // Do nothing.
                // Expect Client that owns this connection 
                // To destroy this instance.
            }
        }

        self.conn_state = next_state;

        Ok(())
    }

    pub fn get_state(&self) -> ConnState
    {
        self.conn_state.clone()
    }
}

// I had more connection tests, but creating them with encryption was remaking the client and loadbalancer code again.
// So removed.

