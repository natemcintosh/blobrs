use crate::app::App;

pub mod app;
pub mod event;
pub mod preview;
pub mod terminal_icons;
pub mod ui;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Initialize Azure Storage Account credentials
    let storage_account = std::env::var("AZURE_STORAGE_ACCOUNT")
        .expect("AZURE_STORAGE_ACCOUNT environment variable not set");
    let access_key = std::env::var("AZURE_STORAGE_ACCESS_KEY")
        .expect("AZURE_STORAGE_ACCESS_KEY environment variable not set");

    ratatui::run(|terminal| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        runtime.block_on(async {
            App::new(storage_account, access_key)
                .await?
                .run(terminal)
                .await
        })
    })
}
