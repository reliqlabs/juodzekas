use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Curve error: {0}")]
    Curve(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Verification error: {0}")]
    Verification(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}
