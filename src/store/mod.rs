pub mod keys;
pub mod users;

use redis::aio::ConnectionManager;

pub type Redis = ConnectionManager;

pub async fn connect(redis_url: &str) -> anyhow::Result<Redis> {
    let client = redis::Client::open(redis_url)?;
    let manager = ConnectionManager::new(client).await?;
    Ok(manager)
}
