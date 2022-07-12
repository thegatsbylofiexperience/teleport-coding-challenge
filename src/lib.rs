#[allow(non_camel_case_types)]

use std::net::{TcpListener, TcpStream};
use std::collections::*;

use std::io::Write;
use std::io::Read;

use std::time::Duration;

pub mod config;

pub struct LoadBalancer
{
    clients         : HashMap<String, Client>, // email address, Client
    server_groups   : HashMap<u32, ServerGroup>, // server group id, ServerGroup
    partial_conns   : Vec<PartialConnection>,
    listener        : std::net::TcpListener,
}

impl LoadBalancer
{
    fn new() -> Result<Self, Box<dyn std::error::Error>>
    {
        let listener = TcpListener::bind("127.0.0.1:443")?;

        listener.set_nonblocking(true)?;

        Ok(Self { clients : HashMap::new(), server_groups : HashMap::new(), partial_conns: vec![], listener })
    }

    pub fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
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

                    self.partial_conns.push(PartialConnection::new(stream));
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

        for (k,v) in self.clients.iter_mut()
        {
            v.poll()?;
        }

        for (k, v) in self.server_groups.iter_mut()
        {
            v.poll()?;
        }

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
                Err(e) =>
                {
                    to_remove.push(i);
                }
            }
        }

        for i in to_remove.iter().rev()
        {
            self.partial_conns.remove(*i);
        }

        // Remove from Vec in reverse order to keep index ordering
        for i in to_complete.iter().rev()
        {
            let par_cxn = self.partial_conns.remove(*i);

            if let Some(id) = par_cxn.client_id()
            {
                if let Some(client) = self.clients.get(&id)
                {
                    if let Some(server_group) = self.server_groups.get(&client.allowed_server_group)
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
                                        // add to client connections list
                                        self.insert_connection(&id, conn);
                                    },
                                    Err(e) =>
                                    {
                                        //TODO: handle errors
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
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

        // cleanup connections 
        self.cleanup_connections();

        Ok(())
    }

    //TODO: LOGGING
    fn cleanup_connections(&mut self)
    {
        let mut to_delete : Vec<usize> = vec![];

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
                    to_delete.push(i);
                }
                _ => {}
            }
        }

        for i in to_delete.iter()
        {
            self.connections.remove(*i);
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum PartialConnState
{
    INIT,
    AUTHENTICATING,
    COMPLETED,
    ERROR,
}

struct PartialConnection
{
    down_stream         : std::net::TcpStream,
    state               : PartialConnState,
    email_address       : Option<String>,
    
}

impl PartialConnection
{
    fn new(down_stream: std::net::TcpStream) -> Self
    {
        Self { down_stream, state: PartialConnState::INIT, email_address: None }
    }

    fn poll(&mut self, ) -> Result<(), Box<dyn std::error::Error>>
    {
        let mut next_state = self.state.clone();

		match self.state
        {
            PartialConnState::INIT =>
            {
                // Handle authentication / authorisation
                // TODO: Actually do authentication and authorization in a later pull req
                // Currently hardcoding 
                self.email_address = Some("first@first.com".to_string());

                next_state = PartialConnState::COMPLETED;
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

enum ConnState
{
    INIT,
    UP_CONNECTED,
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
//    tls                 : rustls::Stream, 
    conn_state          : ConnState,
    upstream_serv_group : u32,
    upstream_serv_id    : u32,
}

impl Connection
{
    fn new(down_stream: std::net::TcpStream, up_stream: std::net::TcpStream, upstream_serv_group: u32, upstream_serv_id: u32) -> Result<Self, Box<dyn std::error::Error>>
    {

        Ok(Self { down_stream, up_stream, conn_state: ConnState::OKAY, upstream_serv_group, upstream_serv_id })
    }

    fn from_partial_connection(partial_cxn: PartialConnection, upstream_serv_group: u32, upstream_serv_id: u32, up_stream_addr: &String) -> Result<Self, Box<dyn std::error::Error>>
    {
        // TODO: Blocking call. Move to call with a timeout or wrap in a thread.
        let mut up_stream = std::net::TcpStream::connect(up_stream_addr)?;

        Self::new(partial_cxn.down_stream, up_stream, upstream_serv_group, upstream_serv_id)
    }

    fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        match self.conn_state
        {
            ConnState::INIT =>
            {
                //TODO: Figure out what to do here -> see next comment block
            },
            ConnState::UP_CONNECTED =>
            {
                //TODO: Figure out what to do here... this is not necessary unless we put the upstream
                //      socket in a thread.
                //      The TcpStream::connect() and connect_timeout() will block for some period.
                //      For now I will use connect_timeout but I think a small amount of async/threading here
                //      would / could make life easier 
            },
            ConnState::OKAY =>
            {
                let mut down_buf : [u8; 2048] = [0; 2048];
                let mut up_buf   : [u8; 2048] = [0; 2048];

                match self.down_stream.read(&mut down_buf)
                {
                    Ok(n) =>
                    {
                        // write buffer back
                        match self.up_stream.write_all(&down_buf[0..n])
                        {
                            Ok(n) =>
                            {
                                // Great!
                            },
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                            {
                                // wait for next poll
                            },
                            Err(e) =>
                            {
                                // TODO: Handle errors
                            }
                        }
                    },
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        // wait for next poll
                    },
                    Err(e) =>
                    {
                        // TODO: handle error types
                        // TODO: log and remove stream

                    }
                }

                match self.up_stream.read(&mut up_buf)
                {
                    Ok(n) =>
                    {
                        // write buffer back
                        match self.down_stream.write_all(&up_buf[0..n])
                        {
                            Ok(n) =>
                            {
                                // Great!
                            },
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                            {
                                // wait for next poll
                            },
                            Err(e) =>
                            {
                                // TODO: Handle errors
                            }
                        }
                    },
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        // wait for next poll
                    },
                    Err(e) =>
                    {
                        // TODO: handle error types
                        // TODO: log and remove stream

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

        Ok(())
    }
}

// TODO: Server health should be put in a thread
// TODO: Server health should complete a connection within some period
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

    fn find_least_connected_and_healthy_server(&self) -> Option<&String>
    {
        if let Some(server_id) = self.find_min_and_healthy()
        {
            return self.server_addrs.get(&server_id);
        }

        None
    }

    fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
    {
        for (k, v) in self.server_health.iter_mut()
        {
            v.poll()?;
        }

        Ok(())
    }
}

#[derive(PartialEq, Eq)]
enum UpstreamState
{
    HEALTHY,
    UNHEALTHY,
}

#[derive(Clone)]
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
                            //TODO: Log
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
                                    if now > (timestamp + 1)
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
                                // TODO: handle error types
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

