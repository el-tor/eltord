use std::error::Error;

pub async fn get_relay_descriptors() -> Result<String, Box<dyn Error>> {
    Ok("relay descriptors".to_string())
}
