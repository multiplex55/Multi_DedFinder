use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("route generation requires a start system")]
    MissingStartSystem,

    #[error("no route candidates matched the configured filters")]
    NoCandidates,
}
