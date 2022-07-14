#![allow(non_camel_case_types, unused_variables)]

use std::net::{TcpListener, TcpStream};
use std::collections::*;

use std::io::{Write, Read};

use std::time::Duration;

use std::sync::Arc;
use rustls;

use x509_parser::prelude::*;

use log::{info, warn, error};

pub mod config;
mod client;
mod server;


pub struct LoadBalancer
{
    clients         : HashMap<String, client::Client>, // email address, Client
    server_groups   : HashMap<u32, server::ServerGroup>, // server group id, ServerGroup
    partial_conns   : Vec<client::PartialConnection>,
    listener        : std::net::TcpListener,
    config          : Arc<rustls::ServerConfig>,
}

impl LoadBalancer
{
    fn new(config: Arc<rustls::ServerConfig>, port: u16) -> Result<Self, Box<dyn std::error::Error>>
    {
        let listener = TcpListener::bind(format!("127.0.0.1:{port}"))?;

		info!("Listening on 127.0.0.1:{port}");
        
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
                    info!("Client Connected!");
                    
					// Set values to ensure the stream is non-blocking
                    // and that data is sent immediately
                    stream.set_read_timeout(Some(Duration::from_millis(1)))?;
                    stream.set_write_timeout(Some(Duration::from_millis(1)))?;
                    stream.set_nonblocking(true)?;
                    stream.set_nodelay(true)?;
	
					let tls_conn = rustls::ServerConnection::new(Arc::clone(&self.config))?;

                    self.partial_conns.push(client::PartialConnection::new(stream, tls_conn));
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
        for (k,v) in self.clients.iter_mut()
        {
            v.poll()?;

            for v in v.cleanup_connections().iter()
            {
                // remove from connection stats
                if let Some(server_group) = self.server_groups.get_mut(&v.get_upstream_server_group())
                {
                    server_group.remove_connection(&v.get_upstream_server_id());
                }

                info!("Client {k}: removing connection from server group: {} id: {} reason: {:?}.", v.get_upstream_server_group(), v.get_upstream_server_group(), v.get_state());
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
						info!("Partial connection is complete and authed to client {:?}", v.client_id());

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
            info!("removing partial connection {}", i);
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
                    if let Some(server_group) = self.server_groups.get_mut(&client.get_server_group())
                    {
                        // get least connected and healthy upstream
                        if let Some(server_id) = server_group.find_min_and_healthy()
                        {
                            if let Some(upstream_addr) = server_group.get_server_address(&server_id)
                            {
                                // create connection
                                match client::Connection::from_partial_connection(par_cxn, client.get_server_group(), server_id, upstream_addr)
                                {
                                    Ok(conn) =>
                                    {
                                        info!("Full connection made: {id} {} {server_id} {upstream_addr}", client.get_server_group());
                                        // Add server connection to server stats
                                        server_group.add_connection(&server_id);
                                        // add to client connections list
                                        self.insert_connection(&id, conn);
                                    },
                                    Err(e) =>
                                    {
                                        error!("Partial Connection conversion failed: {e}");
                                        error!("id: {id} s_group: {} s_id: {server_id} addr: {upstream_addr}", client.get_server_group());
                                    }
                                }
                            }
                            else
                            {
                                error!("No server address found for server id {} in server group {} .. dropping", server_id, client.get_server_group());
                            }
                        }
                        else
                        {
                            error!("No healthy server found in server group {} .. dropping", client.get_server_group());
                        }
                    }
                    else
                    {
                        error!("Server group {} not found on server for client {} .. dropping", client.get_server_group(), id);
                    }
                }
                else
                {
                    error!("Client id {} not found in server records .. dropping", id);
                }
            }
			else
			{
                error!("No email found for complete partial connection.. dropping.");
            }
        }
    }

    fn insert_connection(&mut self, client_id: &String, cxn: client::Connection)
    {
        if let Some(client) = self.clients.get_mut(client_id)
        {
            client.add_connection(cxn);
        }
        else
        {
            error!("Client {client_id} not found to insert connection .. dropping connection")
        }
    }
}


