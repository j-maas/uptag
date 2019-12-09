use updock::TagFetcher;

use env_logger;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    print!("{:?}", TagFetcher::fetch("osixia/openldap")?);
    Ok(())
}
