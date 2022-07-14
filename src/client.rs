#![allow(non_camel_case_types, unused_variables)]

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
        self.connections.push(cxn);
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
    
    for i in 0..8
    {
        let mut down_stream = TcpStream::connect(addr.clone()).unwrap();
        let mut up_stream = TcpStream::connect(addr.clone()).unwrap();

        let mut cxn = Connection::new(down_stream, up_stream, 0, 0).unwrap();

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
                        debug!("Received: {n} bytes");

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
                        debug!("Sent: {n} bytes");

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

#[test]
fn test_connection_both_connected_data_transfer()
{
    let lb_addr: String = "127.0.0.1:25006".into();
    let us_addr: String = "127.0.0.1:25007".into();

    // loadbalancer server listner
    let lb_listener = std::net::TcpListener::bind(lb_addr.clone()).unwrap();
    lb_listener.set_nonblocking(true).unwrap();

    // upstream server listner
    let us_listener = std::net::TcpListener::bind(us_addr.clone()).unwrap();
    us_listener.set_nonblocking(true).unwrap();

    // Client cxn to load balancer
    let mut client = TcpStream::connect(lb_addr).unwrap();
    client.set_read_timeout(Some(Duration::from_millis(1))).unwrap();
    client.set_write_timeout(Some(Duration::from_millis(1))).unwrap();
    client.set_nonblocking(true).unwrap();
    client.set_nodelay(true).unwrap();

    // load balancer cxn to load upstream
    let mut up_stream = TcpStream::connect(us_addr).unwrap();
    up_stream.set_read_timeout(Some(Duration::from_millis(1))).unwrap();
    up_stream.set_write_timeout(Some(Duration::from_millis(1))).unwrap();
    up_stream.set_nonblocking(true).unwrap();
    up_stream.set_nodelay(true).unwrap();

    // client cxn inside the loadbalancer
    let mut down_stream : Option<std::net::TcpStream> = None;
 
    // upstream cxn to loadbalancer
    let mut up_server_stream : Option<std::net::TcpStream> = None;

    loop
    {
        let mut should_break = false;

        for stream_res in lb_listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    strm.set_read_timeout(Some(Duration::from_millis(1))).unwrap();
                    strm.set_write_timeout(Some(Duration::from_millis(1))).unwrap();
                    strm.set_nonblocking(true).unwrap();
                    strm.set_nodelay(true).unwrap();
                    down_stream = Some(strm);
                    should_break = true;
                },
    		    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		    { 
				    break;
    		    },
    		    Err(e) =>
    		    {
                    assert!(false);
			    }
            }
        }
        if should_break
        {
            break;
        }
    }

    loop
    {
        let mut should_break = false;

        for stream_res in us_listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    strm.set_read_timeout(Some(Duration::from_millis(1))).unwrap();
                    strm.set_write_timeout(Some(Duration::from_millis(1))).unwrap();
                    strm.set_nonblocking(true).unwrap();
                    strm.set_nodelay(true).unwrap();
                    up_server_stream = Some(strm);
                    should_break = true;
                },
    		    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		    { 
				    break;
    		    },
    		    Err(e) =>
    		    {
                    assert!(false);
			    }
            }
        }

        if should_break
        {
            break;
        }
    }

    let mut cxn = Connection::new(down_stream.unwrap(), up_stream, 0, 0).unwrap(); 

    client.write_all("HELLO".as_bytes());
    up_server_stream.as_ref().unwrap().write_all("GOODBYE".as_bytes()).unwrap();

    cxn.poll().unwrap();

    let mut buf : [u8; 8] = [0; 8];
    match up_server_stream.as_ref().unwrap().read(&mut buf)
    {
        Ok(n) =>
        {
            assert!(buf[0..n] == *"HELLO".as_bytes());
        },
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
        {
            // wait for next poll
            assert!(false);
        },
        Err(e) =>
        {
            assert!(false);
        }
    }

    let mut buf : [u8; 8] = [0; 8];
    match client.read(&mut buf)
    {
        Ok(n) =>
        {
            assert!(buf[0..n] == *"GOODBYE".as_bytes());
        },
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
        {
            // wait for next poll
            assert!(false);
        },
        Err(e) =>
        {
            assert!(false);
        }
    }

    assert!(cxn.conn_state == ConnState::OKAY);
}

#[test]
fn test_connection_both_connected_client_drops()
{
    let lb_addr: String = "127.0.0.1:25008".into();
    let us_addr: String = "127.0.0.1:25009".into();

    let lb_listener = std::net::TcpListener::bind(lb_addr.clone()).unwrap();
    lb_listener.set_nonblocking(true).unwrap();

    let us_listener = std::net::TcpListener::bind(us_addr.clone()).unwrap();
    us_listener.set_nonblocking(true).unwrap();

    let one_milli = Duration::from_millis(1);
    let mut client : Option<TcpStream> = Some(TcpStream::connect(lb_addr).unwrap());
    client.as_ref().unwrap().set_read_timeout(Some(one_milli.clone())).unwrap();
    client.as_ref().unwrap().set_write_timeout(Some(one_milli.clone())).unwrap();
    client.as_ref().unwrap().set_nonblocking(true).unwrap();
    client.as_ref().unwrap().set_nodelay(true).unwrap();

    let mut up_stream = TcpStream::connect(us_addr).unwrap();
    up_stream.set_read_timeout(Some(one_milli.clone())).unwrap();
    up_stream.set_write_timeout(Some(one_milli.clone())).unwrap();
    up_stream.set_nonblocking(true).unwrap();
    up_stream.set_nodelay(true).unwrap();

    let mut down_stream : Option<std::net::TcpStream> = None;
    let mut up_server_stream : Option<std::net::TcpStream> = None;

    loop
    {
        let mut should_break = false;

        for stream_res in lb_listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    strm.set_read_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_nonblocking(true).unwrap();
                    strm.set_nodelay(true).unwrap();
                    down_stream = Some(strm);
                    should_break = true;
                },
    		    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		    { 
				    break;
    		    },
    		    Err(e) =>
    		    {
                    assert!(false);
			    }
            }
        }
        if should_break
        {
            break;
        }
    }

    loop
    {
        let mut should_break = false;

        for stream_res in us_listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    strm.set_read_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_nonblocking(true).unwrap();
                    strm.set_nodelay(true).unwrap();
                    up_server_stream = Some(strm);
                    should_break = true;
                },
    		    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		    { 
				    break;
    		    },
    		    Err(e) =>
    		    {
                    assert!(false);
			    }
            }
        }

        if should_break
        {
            break;
        }
    }

    let mut cxn = Connection::new(down_stream.unwrap(), up_stream, 0, 0).unwrap(); 

    client.as_ref().unwrap().write_all("HELLO".as_bytes());
    up_server_stream.as_ref().unwrap().write_all("GOODBYE".as_bytes()).unwrap();

    cxn.poll().unwrap();

    client = None;
    
    let mut buf : [u8; 8] = [0; 8];
    match up_server_stream.as_ref().unwrap().read(&mut buf)
    {
        Ok(n) =>
        {
            assert!(buf[0..n] == *"HELLO".as_bytes());
        },
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
        {
            // wait for next poll
            assert!(false);
        },
        Err(e) =>
        {
            assert!(false);
        }
    }

    cxn.poll().unwrap();

    println!("{:?}", cxn.conn_state);
    assert!(cxn.conn_state == ConnState::DOWN_DISCONNECT);
}

#[test]
fn test_connection_both_connected_upstream_drops()
{
    let lb_addr: String = "127.0.0.1:25010".into();
    let us_addr: String = "127.0.0.1:25011".into();

    let lb_listener = std::net::TcpListener::bind(lb_addr.clone()).unwrap();
    lb_listener.set_nonblocking(true).unwrap();

    let us_listener = std::net::TcpListener::bind(us_addr.clone()).unwrap();
    us_listener.set_nonblocking(true).unwrap();

    let one_milli = Duration::from_millis(1);
    let mut client = TcpStream::connect(lb_addr).unwrap();
    client.set_read_timeout(Some(one_milli.clone())).unwrap();
    client.set_write_timeout(Some(one_milli.clone())).unwrap();
    client.set_nonblocking(true).unwrap();
    client.set_nodelay(true).unwrap();

    let mut up_stream = TcpStream::connect(us_addr).unwrap();
    up_stream.set_read_timeout(Some(one_milli.clone())).unwrap();
    up_stream.set_write_timeout(Some(one_milli.clone())).unwrap();
    up_stream.set_nonblocking(true).unwrap();
    up_stream.set_nodelay(true).unwrap();

    let mut down_stream : Option<std::net::TcpStream> = None;
    let mut up_server_stream : Option<std::net::TcpStream> = None;

    loop
    {
        let mut should_break = false;

        for stream_res in lb_listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    strm.set_read_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_nonblocking(true).unwrap();
                    strm.set_nodelay(true).unwrap();
                    down_stream = Some(strm);
                    should_break = true;
                },
    		    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		    { 
				    break;
    		    },
    		    Err(e) =>
    		    {
                    assert!(false);
			    }
            }
        }
        if should_break
        {
            break;
        }
    }

    loop
    {
        let mut should_break = false;

        for stream_res in us_listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    strm.set_read_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_nonblocking(true).unwrap();
                    strm.set_nodelay(true).unwrap();
                    up_server_stream = Some(strm);
                    should_break = true;
                },
    		    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		    { 
				    break;
    		    },
    		    Err(e) =>
    		    {
                    assert!(false);
			    }
            }
        }

        if should_break
        {
            break;
        }
    }

    let mut cxn = Connection::new(down_stream.unwrap(), up_stream, 0, 0).unwrap(); 

    client.write_all("HELLO".as_bytes());
    up_server_stream.as_ref().unwrap().write_all("GOODBYE".as_bytes()).unwrap();

    cxn.poll().unwrap();

    up_server_stream = None;

    let mut buf : [u8; 8] = [0; 8];
    match client.read(&mut buf)
    {
        Ok(n) =>
        {
            assert!(buf[0..n] == *"GOODBYE".as_bytes());
        },
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
        {
            // wait for next poll
            assert!(false);
        },
        Err(e) =>
        {
            assert!(false);
        }
    }

    cxn.poll().unwrap();

    println!("{:?}", cxn.conn_state);
    assert!(cxn.conn_state == ConnState::UP_DISCONNECT);
}

#[test]
fn test_connection_client_connected_upstream_down()
{
    let lb_addr: String = "127.0.0.1:25012".into();
    let us_addr: String = "127.0.0.1:25013".into();

    let lb_listener = std::net::TcpListener::bind(lb_addr.clone()).unwrap();
    lb_listener.set_nonblocking(true).unwrap();
    
    let one_milli = Duration::from_millis(1);
    let mut client = TcpStream::connect(lb_addr).unwrap();
    client.set_read_timeout(Some(one_milli.clone())).unwrap();
    client.set_write_timeout(Some(one_milli.clone())).unwrap();
    client.set_nonblocking(true).unwrap();
    client.set_nodelay(true).unwrap();
    
    let mut down_stream : Option<std::net::TcpStream> = None;
    loop
    {
        let mut should_break = false;

        for stream_res in lb_listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    strm.set_read_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(one_milli.clone())).unwrap();
                    strm.set_nonblocking(true).unwrap();
                    strm.set_nodelay(true).unwrap();
                    down_stream = Some(strm);
                    should_break = true;
                },
    		    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		    { 
				    break;
    		    },
    		    Err(e) =>
    		    {
                    assert!(false);
			    }
            }
        }
        if should_break
        {
            break;
        }
    }

    let mut par_cxn = PartialConnection::new(down_stream.unwrap());

    // Set to  
    par_cxn.poll();

    match Connection::from_partial_connection(par_cxn, 0, 0, &us_addr)
    {
        Ok(cxn) => 
        {
            assert!(false);
        },
        Err(e) =>
        {
            assert!(true);
        }
    }
}
