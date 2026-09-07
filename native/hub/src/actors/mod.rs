pub(crate) mod download_actor;

#[derive(Debug, thiserror::Error)]
pub enum CreateActorsError {
    #[error("failed to resolve application data directory")]
    ResolveDataDirectory(#[from] fluxdown_engine::data_dir::DataDirError),
    #[error("download actor failed")]
    DownloadActor(#[from] download_actor::ActorError),
}

pub async fn create_actors() -> Result<(), CreateActorsError> {
    // Determine the data directory using the shared resolver.
    //
    // Linux:   $XDG_DATA_HOME/fluxdown  (~/.local/share/fluxdown)
    // macOS:   ~/Library/Application Support/fluxdown
    // Windows portable (marker file present): exe directory
    // Windows installed: %LOCALAPPDATA%\FluxDown
    let db_dir = fluxdown_engine::data_dir::resolve_data_dir(None)?;
    download_actor::run(db_dir).await?;
    Ok(())
}
