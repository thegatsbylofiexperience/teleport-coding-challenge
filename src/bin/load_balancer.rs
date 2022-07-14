#![allow(unreachable_code, unused_imports)]

use teleport_coding_challenge::config;
use log::{warn, info, error};
use std::time::Duration;
use simple_logger::SimpleLogger;
use argparse::{ArgumentParser, StoreTrue, Store};

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    SimpleLogger::new().with_level(log::LevelFilter::Debug).init().unwrap();
    
    let mut other_certs = false;
    let mut port : u16  = 8443;
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("TLS 1.3 Upstream Server");
        ap.refer(&mut other_certs).add_option(&["--other"], StoreTrue, "This flag changes the ca that has signed the server cert -- to test authentication with different CAs");
        ap.refer(&mut port).add_option(&["--port"], Store, "The port that the load balancer will listen to on localhost. default: 8443");
        ap.parse_args_or_exit();
    }

    info!("Init Load Balancer!");

    if !other_certs
    {
        info!("Loading normal CA ad server key + cert");
    }
    else
    {
        info!("Loading alternate CA ad server key + cert");
    }

    // load the whole load balancer in with configuration
    // see src/config.rs for more details
    let mut lb = config::load_configuration(port, other_certs)?;

    loop
    {
        match lb.poll()
        {
            Ok(()) => {},
            Err(e) =>
            {
                error!("{e}")                
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    Ok(())
}

