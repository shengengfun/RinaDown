pub(crate) mod download_actor;

#[derive(Debug, thiserror::Error)]
pub enum CreateActorsError {
    #[error("failed to resolve application data directory")]
    ResolveDataDirectory(#[from] rinadown_engine::data_dir::DataDirError),
    #[error("download actor failed")]
    DownloadActor(#[from] download_actor::ActorError),
}

pub async fn create_actors() -> Result<(), CreateActorsError> {
    // Determine the data directory using the shared resolver.
    //
    // Linux:   $XDG_DATA_HOME/rinadown  (~/.local/share/rinadown)
    // macOS:   ~/Library/Application Support/rinadown
    // Windows portable (marker file present): exe directory
    // Windows installed: %LOCALAPPDATA%\RinaDown
    let db_dir = rinadown_engine::data_dir::resolve_data_dir(None)?;
    download_actor::run(db_dir).await?;
    Ok(())
}
