use crate::app::App;

pub mod app;
mod bulk;
mod config;
mod favorites;
pub mod preview;
mod task;
pub mod terminal_icons;
pub mod ui;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // The Azure CLI supplies credentials from the current login session.
    let storage_account = std::env::var("AZURE_STORAGE_ACCOUNT")
        .ok()
        .filter(|account| !account.trim().is_empty());

    ratatui::run(|terminal| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        let result = runtime.block_on(async { App::new(storage_account)?.run(terminal).await });
        // Do not wait for a native file dialog that is still open.
        runtime.shutdown_background();
        result
    })
}
