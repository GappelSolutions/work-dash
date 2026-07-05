#[tokio::main]
async fn main() -> anyhow::Result<()> {
    work_dash_server::run().await
}
