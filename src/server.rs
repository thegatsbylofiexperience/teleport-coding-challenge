#![allow(non_camel_case_types, unused_variables)]

use std::net::{TcpListener, TcpStream};
use std::collections::*;

use std::io::{Write, Read};

use std::time::Duration;

use std::sync::Arc;
use rustls;

use x509_parser::prelude::*;

use log::{info, warn, error};

// TODO: Server health should be put in a thread
// TODO: Server health should complete a connection within some period
// TODO: Connections need to be accounted for.
pub struct ServerGroup
{
    id              : u32,
    server_addrs    : HashMap<u32, String>,
    cxn_cntr        : HashMap<u32, usize>,
    server_health   : HashMap<u32, HealthChecker>,
}

impl ServerGroup
{
    pub fn new(id : u32) -> Self
    {
        Self { id, server_addrs: HashMap::new(), cxn_cntr: HashMap::new(), server_health: HashMap::new() }
    }

    pub fn add_connection(&mut self, id: &u32)
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

    pub fn remove_connection(&mut self, id: &u32)
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

    pub fn add_server(&mut self, serv_id: u32, addr: String)
    {
        self.server_addrs.insert(serv_id, addr.clone());
        self.server_health.insert(serv_id, HealthChecker::new(serv_id, addr));
    }

    pub fn get_server_address(&self, serv_id: &u32) -> Option<&String>
    {
        self.server_addrs.get(serv_id)
    }

    pub fn find_min(&self) -> Option<u32>
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

    pub fn find_min_and_healthy(&self) -> Option<u32>
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

    pub fn poll(&mut self) -> Result<(), Box<dyn std::error::Error>>
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

pub struct HealthChecker
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
    pub fn new(server_id: u32, address: String) ->  Self
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
                if now > idle_ts + 30
                {
                    // Connect
                    // TODO: use connect_timeout
                    // TODO: Or use thread
                    match TcpStream::connect(self.address.clone())
                    {
                        Ok(stream) =>
                        {
                            stream.set_read_timeout(Some(Duration::from_millis(1)))?;
                            stream.set_write_timeout(Some(Duration::from_millis(1)))?;
                            stream.set_nonblocking(true)?;
                            stream.set_nodelay(true)?;

                            self.up_stream = Some(stream);

                            next_state = PingState::CONNECTED;
                        },
                        Err(e) =>
                        {
                            self.upstream_state = UpstreamState::UNHEALTHY;
                            next_state = PingState::IDLE(now);
                            error!("{} set to UNHEALTHY", self.server_id);
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
                        Ok(()) =>
                        {
                            next_state = PingState::PING_SENT(now);
                            info!("{} sent ping", self.server_id);
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
                            
                            error!("{} set to UNHEALTHY", self.server_id);
                        }
                    }
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
                    error!("{} set to UNHEALTHY", self.server_id);
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
                                        error!("{} set to UNHEALTHY", self.server_id);
                                    }
                                    else
                                    {
                                        self.upstream_state = UpstreamState::HEALTHY;
                                        info!("{} set to HEALTHY", self.server_id);
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
                                
                                error!("{} set to UNHEALTHY", self.server_id);
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

