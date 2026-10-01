use crate::app::App;

pub mod app;
mod favorites;
pub mod preview;
pub mod terminal_icons;
pub mod ui;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // The Azure CLI supplies credentials from the current login session.
    let storage_account = std::env::var("AZURE_STORAGE_ACCOUNT")
        .ok()
        .filter(|account| !account.trim().is_empty())
        .ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "Set AZURE_STORAGE_ACCOUNT to your Azure Storage account name before starting blobrs.\n\
                 Bash/zsh: export AZURE_STORAGE_ACCOUNT=\"your_storage_account_name\"\n\
                 PowerShell: $env:AZURE_STORAGE_ACCOUNT = \"your_storage_account_name\"\n\
                 Add this setting to your shell profile to use blobrs from any directory.\n\
                 Sign in with `az login` (or `az login --identity`) before connecting.\n\
                 blobrs does not load .env files; `just run` loads the repository's .env for development."
            )
        })?;

    ratatui::run(|terminal| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        runtime.block_on(async { App::new(storage_account).await?.run(terminal).await })
    })
}
