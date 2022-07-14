#![allow(unreachable_code, unused_imports)]

use teleport_coding_challenge::config;
use log::{warn, info, error};
use std::time::Duration;
use simple_logger::SimpleLogger;

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    SimpleLogger::new().with_level(log::LevelFilter::Debug).init().unwrap();

    info!("Init LB!");

    let mut lb = config::load_configuration()?;

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

