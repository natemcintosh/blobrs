use crate::app::App;
use object_store::azure::MicrosoftAzureBuilder;
use std::sync::Arc;

pub mod app;
pub mod event;
pub mod terminal_icons;
pub mod ui;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Initialize Azure Blob Storage
    let storage_account = std::env::var("AZURE_STORAGE_ACCOUNT")
        .expect("AZURE_STORAGE_ACCOUNT environment variable not set");
    let container_name = std::env::var("AZURE_CONTAINER_NAME")
        .expect("AZURE_CONTAINER_NAME environment variable not set");
    let access_key = std::env::var("AZURE_STORAGE_ACCESS_KEY")
        .expect("AZURE_STORAGE_ACCESS_KEY environment variable not set");

    let azure_client = MicrosoftAzureBuilder::new()
        .with_account(storage_account)
        .with_container_name(container_name)
        .with_access_key(access_key)
        .build()?;

    let object_store = Arc::new(azure_client);

    let terminal = ratatui::init();
    let result = App::new(object_store).await?.run(terminal).await;
    ratatui::restore();
    result
}
