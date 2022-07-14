#![allow(non_camel_case_types, unused_variables)]

use std::net::{TcpListener, TcpStream};
use std::collections::*;

use std::io::Write;
use std::io::Read;

use std::time::Duration;

use std::sync::Arc;
use rustls;

use x509_parser::prelude::*;

pub mod config;

pub struct LoadBalancer
{
    clients         : HashMap<String, Client>, // email address, Client
    server_groups   : HashMap<u32, ServerGroup>, // server group id, ServerGroup
    partial_conns   : Vec<PartialConnection>,
    listener        : std::net::TcpListener,
    config          : Arc<rustls::ServerConfig>,
}

impl LoadBalancer
{
    fn new(config: Arc<rustls::ServerConfig>) -> Result<Self, Box<dyn std::error::Error>>
    {
        let listener = TcpListener::bind("127.0.0.1:443")?;

        listener.set_nonblocking(true)?;

        Ok(Self { clients : HashMap::new(), server_groups : HashMap::new(), partial_conns: vec![], listener, config })
    }

    pub fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        self.handle_listener()?;

        self.handle_clients()?;

        self.handle_server_groups()?;

        self.handle_partial_connections();

        Ok(())
    }

    fn handle_listener(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        for stream_res in self.listener.incoming()
        {
			match stream_res
			{
				Ok(stream) =>
				{
					// Handle new stream
                    
                    // Set values to ensure the stream is non-blocking
                    // and that data is sent immediately
                    let point_one_milli = Duration::from_micros(100);

                    stream.set_read_timeout(Some(point_one_milli.clone()))?;
                    stream.set_write_timeout(Some(point_one_milli.clone()))?;
                    stream.set_nonblocking(true)?;
                    stream.set_nodelay(true)?;
	
					let tls_conn = rustls::ServerConnection::new(Arc::clone(&self.config))?;

                    self.partial_conns.push(PartialConnection::new(stream, tls_conn));
				},
    			Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    			{
					// Do nothing we will 
					break;
    			},
    			Err(e) =>
    			{
					return Err(e.into());
				}
			}
        }

        Ok(())
    }

    fn handle_clients(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        for (_k,v) in self.clients.iter_mut()
        {
            v.poll()?;

            for v in v.cleanup_connections().iter()
            {
                // remove from connection stats
                if let Some(server_group) = self.server_groups.get_mut(&v.upstream_serv_group)
                {
                    server_group.remove_connection(&v.upstream_serv_id);
                }
            }
        }

        Ok(())
    }

    fn handle_server_groups(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        for (_k, v) in self.server_groups.iter_mut()
        {
            v.poll()?;
        }

        Ok(())
    }

    fn handle_partial_connections(&mut self)
    {
        let mut to_remove : Vec<usize> = vec![];
        let mut to_complete : Vec<usize> = vec![];

        for (i,v) in self.partial_conns.iter_mut().enumerate()
        {
            match v.poll()
            {
                Ok(()) =>
                {
                    if v.is_completed()
                    {
                        to_complete.push(i);
                    }
                },
                Err(_e) =>
                {
                    to_remove.push(i);
                }
            }
        }

        for i in to_remove.iter().rev()
        {
            self.partial_conns.remove(*i);
        }

        // Remove from Vec in reverse order
        // to keep index ordering intact
        for i in to_complete.iter().rev()
        {
            let par_cxn = self.partial_conns.remove(*i);

            if let Some(id) = par_cxn.client_id()
            {
                if let Some(client) = self.clients.get(&id)
                {
                    if let Some(server_group) = self.server_groups.get_mut(&client.allowed_server_group)
                    {
                        // get least connected and healthy upstream
                        if let Some(server_id) = server_group.find_min_and_healthy()
                        {
                            if let Some(upstream_addr) = server_group.server_addrs.get(&server_id)
                            {
                                // create connection
                                match Connection::from_partial_connection(par_cxn, client.allowed_server_group, server_id, upstream_addr)
                                {
                                    Ok(conn) =>
                                    {
                                        // Add server connection to server stats
                                        server_group.add_connection(&server_id);
                                        // add to client connections list
                                        self.insert_connection(&id, conn);
                                    },
                                    Err(_e) =>
                                    {
                                        //TODO: Log errors
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn insert_connection(&mut self, client_id: &String, cxn: Connection)
    {
        if let Some(client) = self.clients.get_mut(client_id)
        {
            client.connections.push(cxn);
        }
        else
        {
            // TODO: Log / handle error
        }
    }
}

struct Client
{
    email                : String,
    connections          : Vec<Connection>,
    cxn_time             : i64,
    cxn_cnt              : usize,
    allowed_server_group : u32,
}

impl Client
{
    fn new(email: String, allowed_server_group: u32) -> Self
    {
        Self { email, connections: vec![], cxn_time: i64::MIN, cxn_cnt: 0,  allowed_server_group }
    }

    fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        // poll connections
		for cxn in self.connections.iter_mut()
        {
            cxn.poll()?;
        }

        Ok(())
    }

    fn cleanup_connections(&mut self) -> Vec<Connection>
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
            0 => { cxn.conn_state = ConnState::INIT; },
            1 => { cxn.conn_state = ConnState::UP_CONNECTED; },
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
struct PartialConnection
{
    down_stream         : std::net::TcpStream,
    tls_conn            : rustls::ServerConnection,
    state               : PartialConnState,
    email_address       : Option<String>,
    
}

impl PartialConnection
{
    fn new(down_stream: std::net::TcpStream, tls_conn: rustls::ServerConnection) -> Self
    {
        Self { down_stream, tls_conn, state: PartialConnState::INIT, email_address: None }
    }

    fn poll(&mut self, ) -> Result<(), Box<dyn std::error::Error>>
    {
        let mut next_state = self.state.clone();

		match self.state
        {
            PartialConnState::INIT =>
            {
                // Handle authentication / authorisation
                if self.tls_conn.is_handshaking()
                {
				    if self.tls_conn.wants_read()
                    {
                        match self.tls_conn.read_tls(&mut self.down_stream)
                        {
                            Ok(n) =>
                            {
                                if n == 0
                                {
                                    next_state = PartialConnState::ERROR;
                                }
                                else
                                {
                                    if let Ok(_io_state) = self.tls_conn.process_new_packets()
                                    {
                                        // we are handshaking so we want to process the handshake packets 
                                    }
                                    else
                                    {
                                        next_state = PartialConnState::ERROR;
                                    }
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

                    if self.tls_conn.wants_write()
                    {
                        match self.tls_conn.write_tls(&mut self.down_stream)
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
                                self.email_address = Some(email);
                                next_state = PartialConnState::COMPLETED;
                            },
                            Err(_e) =>
                            {
                                next_state = PartialConnState::ERROR;
                            }
                        }
                    }
                    else
                    {
                        next_state = PartialConnState::ERROR;
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


    fn is_completed(&self) -> bool
    {
        return self.state == PartialConnState::COMPLETED;
    }

    fn client_id(&self) -> Option<String>
    {
        self.email_address.clone()
    }
}

fn extract_email_from_cert(cert: &Vec<u8>) -> Result<String, Box<dyn std::error::Error>>
{
    let (rem, cert) = X509Certificate::from_der(&cert[..])?;
    if let Some(email_ext) = cert.get_extension_unique(&oid_registry::OID_PKCS9_EMAIL_ADDRESS)?
    {
        return Ok(String::from_utf8(email_ext.value.to_vec())?);
    }
    else
    {
        return Err("No email address found".into());
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum ConnState
{
    OKAY,
    UP_DISCONNECT,
    UP_TIMEOUT,
    DOWN_DISCONNECT,
    DOWN_TIMEOUT,
    DOWN_ENC_ERR,
}

struct Connection
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
    fn new(down_stream: std::net::TcpStream, up_stream: std::net::TcpStream, tls_conn: rustls::ServerConnection, upstream_serv_group: u32, upstream_serv_id: u32) -> Result<Self, Box<dyn std::error::Error>>
    {

        Ok(Self { down_stream, up_stream, tls_conn, conn_state: ConnState::OKAY, upstream_serv_group, upstream_serv_id })
    }

    fn from_partial_connection(partial_cxn: PartialConnection, upstream_serv_group: u32, upstream_serv_id: u32, up_stream_addr: &String) -> Result<Self, Box<dyn std::error::Error>>
    {
        // TODO: Blocking call. Move to call with a timeout or wrap in a thread.
        let up_stream = std::net::TcpStream::connect(up_stream_addr)?;

        Self::new(partial_cxn.down_stream, up_stream, partial_cxn.tls_conn, upstream_serv_group, upstream_serv_id)
    }

    fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        let mut next_state = self.conn_state.clone();

        match self.conn_state
        {
            ConnState::OKAY =>
            {
                match self.tls_conn.read_tls(&mut self.down_stream)
                {
                    Ok(n) =>
                    {
                        if n == 0
                        {
                            next_state = ConnState::DOWN_DISCONNECT;
                        }
                        else
                        {
					        if let Ok(io_state) = self.tls_conn.process_new_packets()
							{
					            if io_state.plaintext_bytes_to_read() > 0
								{
                					let mut buf = Vec::new();
					                buf.resize(io_state.plaintext_bytes_to_read(), 0u8);

                					self.tls_conn.reader().read_exact(&mut buf)
                    				.unwrap();

                                    // write buffer back
                                    match self.up_stream.write_all(&buf)
                                    {
                                        Ok(()) =>
                                        {
                                            // Great!
                                        },
                                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                                        {
                                            // wait for next poll
                                        },
                                        Err(_e) =>
                                        {
                                            next_state = ConnState::UP_DISCONNECT;
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        // wait for next poll
                    },
                    Err(_e) =>
                    {
                        next_state = ConnState::DOWN_DISCONNECT;
                    }
                }

                let mut up_buf   : [u8; 2048] = [0; 2048];
                match self.up_stream.read(&mut up_buf)
                {
                    Ok(n) =>
                    {
                        if n == 0
                        {
                            next_state = ConnState::UP_DISCONNECT;
                        }
                        else
                        {
                            self.tls_conn.writer().write_all(&up_buf[0..n]).unwrap();

                            // write buffer back
                            match self.tls_conn.write_tls(&mut self.down_stream)
                            {
                                Ok(_n) =>
                                {
                                    if n == 0
                                    {
                                        next_state = ConnState::UP_DISCONNECT;
                                    }

                                    // otherwise Great!
                                },
                                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                                {
                                    // wait for next poll
                                },
                                Err(_e) =>
                                {
                                    next_state = ConnState::DOWN_DISCONNECT;
                                }
                            }
                        }
                    },
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        // wait for next poll
                    },
                    Err(_e) =>
                    {
                        next_state = ConnState::UP_DISCONNECT;
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
        println!("CXN NS: {:?} CurrState: {:?}", next_state, self.conn_state);
        self.conn_state = next_state;

        Ok(())
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

    let point_one_milli = Duration::from_micros(100);

    // Client cxn to load balancer
    let mut client = TcpStream::connect(lb_addr).unwrap();
    client.set_read_timeout(Some(point_one_milli.clone())).unwrap();
    client.set_write_timeout(Some(point_one_milli.clone())).unwrap();
    client.set_nonblocking(true).unwrap();
    client.set_nodelay(true).unwrap();

    // load balancer cxn to load upstream
    let mut up_stream = TcpStream::connect(us_addr).unwrap();
    up_stream.set_read_timeout(Some(point_one_milli.clone())).unwrap();
    up_stream.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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
                    strm.set_read_timeout(Some(point_one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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
                    strm.set_read_timeout(Some(point_one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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

    let point_one_milli = Duration::from_micros(100);
    let mut client : Option<TcpStream> = Some(TcpStream::connect(lb_addr).unwrap());
    client.as_ref().unwrap().set_read_timeout(Some(point_one_milli.clone())).unwrap();
    client.as_ref().unwrap().set_write_timeout(Some(point_one_milli.clone())).unwrap();
    client.as_ref().unwrap().set_nonblocking(true).unwrap();
    client.as_ref().unwrap().set_nodelay(true).unwrap();

    let mut up_stream = TcpStream::connect(us_addr).unwrap();
    up_stream.set_read_timeout(Some(point_one_milli.clone())).unwrap();
    up_stream.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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
                    strm.set_read_timeout(Some(point_one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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
                    strm.set_read_timeout(Some(point_one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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

    let point_one_milli = Duration::from_micros(100);
    let mut client = TcpStream::connect(lb_addr).unwrap();
    client.set_read_timeout(Some(point_one_milli.clone())).unwrap();
    client.set_write_timeout(Some(point_one_milli.clone())).unwrap();
    client.set_nonblocking(true).unwrap();
    client.set_nodelay(true).unwrap();

    let mut up_stream = TcpStream::connect(us_addr).unwrap();
    up_stream.set_read_timeout(Some(point_one_milli.clone())).unwrap();
    up_stream.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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
                    strm.set_read_timeout(Some(point_one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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
                    strm.set_read_timeout(Some(point_one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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
    
    let point_one_milli = Duration::from_micros(100);
    let mut client = TcpStream::connect(lb_addr).unwrap();
    client.set_read_timeout(Some(point_one_milli.clone())).unwrap();
    client.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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
                    strm.set_read_timeout(Some(point_one_milli.clone())).unwrap();
                    strm.set_write_timeout(Some(point_one_milli.clone())).unwrap();
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

// TODO: Server health should be put in a thread
// TODO: Server health should complete a connection within some period
// TODO: Connections need to be accounted for.
struct ServerGroup
{
    id              : u32,
    server_addrs    : HashMap<u32, String>,
    cxn_cntr        : HashMap<u32, usize>,
    server_health   : HashMap<u32, HealthChecker>,
}

impl ServerGroup
{
    fn new(id : u32) -> Self
    {
        Self { id, server_addrs: HashMap::new(), cxn_cntr: HashMap::new(), server_health: HashMap::new() }
    }

    fn add_connection(&mut self, id: &u32)
    {
        if let Some(cxn_cnt) = self.cxn_cntr.get_mut(id)
        {
            *cxn_cnt += 1;
        }
        else
        {
            self.cxn_cntr.insert(*id, 1);
        }
    }

    fn remove_connection(&mut self, id: &u32)
    {
        if let Some(cxn_cnt) = self.cxn_cntr.get_mut(id)
        {
            if cxn_cnt > &mut 0
            {
                *cxn_cnt -= 1;
            }
            else
            {
                // should not happen
                // probably would log if not a coding challenge
            }
        }
        else
        {
            // should not happen
            // probably would log if not a coding challenge
        }
    }

    fn find_min(&self) -> Option<u32>
    {
        // Not all servers have a connection yet
        if self.cxn_cntr.len() != self.server_addrs.len()
        {
            let server_id_set : HashSet<&u32> = self.server_addrs.keys().collect();
            let cxn_id_set    : HashSet<&u32> = self.cxn_cntr.keys().collect();

            // return the first server id that is not in the cxn_id_set but is in the server_id_set
            for id in server_id_set.difference(&cxn_id_set)
            {
                return Some(**id);
            }
        }

        // All servers are in the list.
        // Find the least connected server.
        let mut min_conns = usize::MAX;
        let mut min_id : Option<u32> = None;
        for (id, num_conns) in self.cxn_cntr.iter()
        {
            if min_conns > *num_conns
            {
                min_conns   = *num_conns;
                min_id      = Some(*id);
            }
        }

        min_id
    }

    fn find_min_and_healthy(&self) -> Option<u32>
    {
        // Not all servers have a connection yet
        if self.cxn_cntr.len() != self.server_addrs.len()
        {
            let server_id_set : HashSet<&u32> = self.server_addrs.keys().collect();
            let cxn_id_set    : HashSet<&u32> = self.cxn_cntr.keys().collect();

            // return the first server id that is not in the cxn_id_set but is in the server_id_set
            for id in server_id_set.difference(&cxn_id_set)
            {
                if let Some(health_check) = self.server_health.get(*id)
                {
                    if health_check.is_healthy()
                    {
                        return Some(**id);
                    }
                }
            }
        }

        // All servers are in the list.
        // Find the least connected server that is also healthy
        let mut min_conns = usize::MAX;
        let mut min_id : Option<u32> = None;
        for (id, num_conns) in self.cxn_cntr.iter()
        {
            if let Some(health) = self.server_health.get(id)
            {
                if min_conns > *num_conns && health.is_healthy()
                {
                    min_conns   = *num_conns;
                    min_id      = Some(*id);
                }
            }
        }

        min_id
    }

    fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        for (_k, v) in self.server_health.iter_mut()
        {
            v.poll()?;
        }

        Ok(())
    }
}

#[test]
fn test_server_group_find_min()
{
    let mut sg = ServerGroup::new(0);

    for i in 0..5
    {
        sg.server_addrs.insert(i, "".into());
    }

    let mut cnt : usize = 0;
    for i in 0..10
    {
        if let Some(id) = sg.find_min()
        {
            cnt += 1;

            sg.add_connection(&id);
        }

    }

    assert!(cnt == 10);

    for i in 0..5
    {
        if let Some(cnt) = sg.cxn_cntr.get(&i)
        {
            assert!(cnt == &2);
        }
        else
        {
            // ID should be there
            assert!(false);
        }
    }
}

#[test]
fn test_server_group_add_and_remove_connections()
{
    let mut sg = ServerGroup::new(0);

    for i in 0..5
    {
        sg.server_addrs.insert(i, "".into());
    }

    let mut cnt : usize = 0;
    for i in 0..10
    {
        if let Some(id) = sg.find_min()
        {
            cnt += 1;

            sg.add_connection(&id);
        }

    }

    for i in 0..5
    {
        sg.remove_connection(&i)
    }

    assert!(cnt == 10);

    for i in 0..5
    {
        if let Some(cnt) = sg.cxn_cntr.get(&i)
        {
            assert!(cnt == &1);
        }
        else
        {
            // ID should be there
            assert!(false);
        }
    }
}

#[test]
fn test_server_group_find_min_and_healthy()
{

    let mut sg = ServerGroup::new(0);

    for i in 0..5
    {
        sg.server_addrs.insert(i, "".into());
        sg.server_health.insert(i, HealthChecker::new(i, "".into()));
    }

    let mut cnt : usize = 0;
    for i in 0..10
    {
        if let Some(id) = sg.find_min_and_healthy()
        {
            cnt += 1;

            sg.add_connection(&id);
        }

    }

    assert!(cnt == 10);

    for i in 0..5
    {
        if let Some(cnt) = sg.cxn_cntr.get(&i)
        {
            assert!(cnt == &2);
        }
        else
        {
            // ID should be there
            assert!(false);
        }
    }
}

#[test]
fn test_server_group_find_min_and_some_unhealthy()
{

    let mut sg = ServerGroup::new(0);

    for i in 0..5
    {
        sg.server_addrs.insert(i, "".into());

        let mut hc =  HealthChecker::new(i, "".into());

        hc.upstream_state = UpstreamState::UNHEALTHY;

        sg.server_health.insert(i, hc);
    }

    let mut cnt : usize = 0;
    for i in 0..10
    {
        if let Some(id) = sg.find_min_and_healthy()
        {
            cnt += 1;

            sg.add_connection(&id);
        }

    }

    assert!(cnt == 0);

    for i in 0..5
    {
        if let Some(cnt) = sg.cxn_cntr.get(&i)
        {
            assert!(false);
        }
        else
        {
            // ID should be there
            assert!(true);
        }
    }
}

#[test]
fn test_server_group_find_min_and_all_unhealthy()
{

    let mut sg = ServerGroup::new(0);

    for i in 0..5
    {
        sg.server_addrs.insert(i, "".into());
        sg.server_health.insert(i, HealthChecker::new(i, "".into()));
    }
    
    for i in 5..10
    {
        sg.server_addrs.insert(i, "".into());

        let mut hc =  HealthChecker::new(i, "".into());

        hc.upstream_state = UpstreamState::UNHEALTHY;

        sg.server_health.insert(i, hc);
    }

    let mut cnt : usize = 0;
    for i in 0..10
    {
        if let Some(id) = sg.find_min_and_healthy()
        {
            cnt += 1;

            sg.add_connection(&id);
        }

    }

    assert!(cnt == 10);

    for i in 0..5
    {
        if let Some(cnt) = sg.cxn_cntr.get(&i)
        {
            assert!(cnt == &2);
        }
        else
        {
            // ID should be there
            assert!(false);
        }
    }
    
    for i in 5..10
    {
        if let Some(cnt) = sg.cxn_cntr.get(&i)
        {
            assert!(false);
        }
        else
        {
            assert!(true);
        }
    }
}

#[derive(PartialEq, Eq)]
enum UpstreamState
{
    HEALTHY,
    UNHEALTHY,
}

#[derive(Clone, PartialEq, Eq)]
enum PingState
{
    IDLE(i64),
    CONNECTED,
    PING_SENT(i64),
}

struct HealthChecker
{
    server_id       : u32,
    address         : String,
//    cxn_cnt         : usize,
    up_stream       : Option<std::net::TcpStream>,
    ping_state      : PingState,
    upstream_state  : UpstreamState,
}

impl HealthChecker
{
    fn new(server_id: u32, address: String) ->  Self
    {
        Self { server_id, address, up_stream : None, ping_state : PingState::IDLE(0), upstream_state : UpstreamState::HEALTHY }
    }

    fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        let now = chrono::Utc::now().timestamp();

        let mut next_state = self.ping_state.clone();

        match self.ping_state
        {
            PingState::IDLE(idle_ts) =>
            {
                println!("{} {}",  idle_ts / 30,  now / 30);
                if idle_ts / 30 != now / 30
                {
                    // Connect
                    // TODO: use connect_timeout
                    // TODO: Or use thread
                    match TcpStream::connect(self.address.clone())
                    {
                        Ok(stream) =>
                        {
                            let point_one_milli = Duration::from_micros(100);
                            stream.set_read_timeout(Some(point_one_milli.clone()))?;
                            stream.set_write_timeout(Some(point_one_milli.clone()))?;
                            stream.set_nonblocking(true)?;
                            stream.set_nodelay(true)?;

                            self.up_stream = Some(stream);

                            next_state = PingState::CONNECTED;
                        },
                        Err(e) =>
                        {
                            println!("{e}");
                            self.upstream_state = UpstreamState::UNHEALTHY;
                            // keep state as idle
                        }
                    }
                }

                // keep state as idle
            },
            PingState::CONNECTED =>
            {
                if let Some(stream) = &mut self.up_stream
                {
                    // Send ping
                    match stream.write_all(&"PING".as_bytes())
                    {
                        Ok(n) =>
                        {

                        },
                        Err(e) =>
                        {
                            // would block

                            // others -> 
                        }
                    }

                    next_state = PingState::PING_SENT(now);
                }
            },
            PingState::PING_SENT(timestamp) =>
            {
                if now > (timestamp + 1)
                {
                    // Out of time for pong
                    // mark server as unhealthy
                    self.upstream_state = UpstreamState::UNHEALTHY;
                    self.up_stream = None;
                    next_state = PingState::IDLE(now);
                }
                else
                {
                    if let Some(stream) = &mut self.up_stream
                    {
                        let mut buf : [u8; 16] = [0; 16];
                        // poll and wait

                        match stream.read(&mut buf)
                        {
                            Ok(n) =>
                            {
                                if n == 4 && &buf[0..n] == "PONG".as_bytes()
                                {
                                    // if/when pong received check timestamp
                                    if now > timestamp
                                    {
                                        // if pong not received within 1 second
                                        // mark unhealthy
                                        self.upstream_state = UpstreamState::UNHEALTHY;
                                    }
                                    else
                                    {
                                        self.upstream_state = UpstreamState::HEALTHY;
                                    }
                                }
                                
                                self.up_stream = None;
                                next_state = PingState::IDLE(now);
                            },
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock=>
                            {
                                // wait for next poll
                            },
                            Err(e) =>
                            {
                                // Connection is in error, as such, it is 
                                // UNHEALTHY -> set it as such
                                self.upstream_state = UpstreamState::UNHEALTHY;
                                self.up_stream = None;
                                next_state = PingState::IDLE(now);
                            }
                        }
                    }
                }
            }
        }

        self.ping_state = next_state;

        Ok(())
    }

    fn is_healthy(&self) -> bool
    {
        return self.upstream_state == UpstreamState::HEALTHY
    }
}

//Server Group Tests
#[test]
fn test_server_health_not_listening()
{
    let mut hc = HealthChecker::new(0, "127.0.0.1:25001".into());

    hc.poll().unwrap();

    assert!(hc.upstream_state == UpstreamState::UNHEALTHY);
}

#[test]
fn test_server_health_connect()
{
    let addr: String = "127.0.0.1:25002".into();

    let listener = std::net::TcpListener::bind(addr.clone()).unwrap();
    listener.set_nonblocking(true).unwrap();

    let mut hc = HealthChecker::new(0, addr);

    let mut stream : Option<TcpStream> = None;

    for stream_res in listener.incoming()
    {
		match stream_res
		{
		    Ok(strm) =>
			{
                stream = Some(strm);
            },
    		Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		{
				// Do nothing we will 
				break;
    		},
    		Err(e) =>
    		{
                assert!(false);
			}
        }
    }

    hc.poll().unwrap();

    assert!(hc.upstream_state == UpstreamState::HEALTHY);
}

#[test]
fn test_server_health_reply_in_time()
{
    let addr: String = "127.0.0.1:25003".into();

    let listener = std::net::TcpListener::bind(addr.clone()).unwrap();
    listener.set_nonblocking(true).unwrap();

    let mut hc = HealthChecker::new(0, addr);

    hc.poll().unwrap();

    let mut stream : Option<TcpStream> = None;

    loop
    {
        let mut should_break = false;

        for stream_res in listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    stream = Some(strm);
                    should_break = true;
                },
    		    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		    { 
				    break;
    		    },
    		    Err(e) =>
    		    {
                    // TODO: Log Error
				    //return Err(e.into());
			    }
            }
        }

        if should_break
        {
            break;
        }
    }

    hc.poll().unwrap();

    assert!(hc.upstream_state == UpstreamState::HEALTHY);

    let mut buf : [u8; 4] = [0; 4];
    match stream.as_ref().unwrap().read(&mut buf)
    {
        Ok(n) =>
        {
            if n == 4 && buf[0..n] == *"PING".as_bytes()
            {
                // Send back "PONG"
                stream.as_ref().unwrap().write_all("PONG".as_bytes()).unwrap();
            }
        },
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
        {
            // wait for next poll
        },
        Err(e) =>
        {
            assert!(false);
        }
    }

    hc.poll().unwrap();

    assert!(hc.upstream_state == UpstreamState::HEALTHY);
}

#[test]
fn test_server_health_reply_out_of_time()
{
    let addr: String = "127.0.0.1:25004".into();

    let listener = std::net::TcpListener::bind(addr.clone()).unwrap();
    listener.set_nonblocking(true).unwrap();

    let mut hc = HealthChecker::new(0, addr);

    hc.poll().unwrap();

    let mut stream : Option<TcpStream> = None;

    loop
    {
        let mut should_break = false;

        for stream_res in listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    stream = Some(strm);
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

    hc.poll().unwrap();

    assert!(hc.upstream_state == UpstreamState::HEALTHY);

    let period = std::time::Duration::from_millis(1100);
    std::thread::sleep(period);

    let mut buf : [u8; 4] = [0; 4];
    match stream.as_ref().unwrap().read(&mut buf)
    {
        Ok(n) =>
        {
            if n == 4 && buf[0..n] == *"PING".as_bytes()
            {
                // Send back "PONG"
                stream.as_ref().unwrap().write_all("PONG".as_bytes()).unwrap();
            }
        },
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
        {
            // wait for next poll
        },
        Err(e) =>
        {
            assert!(false);
        }
    }

    hc.poll().unwrap();

    assert!(hc.upstream_state == UpstreamState::UNHEALTHY);
}

#[test]
fn test_server_health_reply_disconnect_from_upstream()
{
    let addr: String = "127.0.0.1:25005".into();

    let listener = std::net::TcpListener::bind(addr.clone()).unwrap();
    listener.set_nonblocking(true).unwrap();

    let mut hc = HealthChecker::new(0, addr);

    hc.poll().unwrap();

    let mut stream : Option<TcpStream> = None;

    loop
    {
        let mut should_break = false;

        for stream_res in listener.incoming()
        {
		    match stream_res
		    {
		        Ok(strm) =>
			    {
                    stream = Some(strm);
                    should_break = true;
                },
    		    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
    		    { 
				    break;
    		    },
    		    Err(e) =>
    		    {
                    // TODO: Log Error
				    //return Err(e.into());
			    }
            }
        }

        if should_break
        {
            break;
        }
    }

    hc.poll().unwrap();

    assert!(hc.upstream_state == UpstreamState::HEALTHY);

    stream = None;

    hc.poll().unwrap();

    assert!(hc.upstream_state == UpstreamState::UNHEALTHY);
}

