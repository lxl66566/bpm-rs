use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    bpm::run().await
}
