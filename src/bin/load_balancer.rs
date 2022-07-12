
use teleport_coding_challenge::config;

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    let mut lb = config::load_configuration()?;

    loop
    {
        match lb.poll()
        {
            Ok(()) => {},
            Err(e) =>
            {
                
            }
        }
    }

    Ok(())
}

