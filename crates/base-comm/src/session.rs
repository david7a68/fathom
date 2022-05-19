use async_trait::async_trait;
use rand::{thread_rng, RngCore};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The provided session token has either expired, or is otherwise invalid.
    #[error("the provided token is invalid, it may have expired")]
    InvalidToken,

    /// The provided user credentials are not valid.
    #[error("the provided user credentials are invalid")]
    InvalidCredential,
}

// 256-bit session token
pub struct SessionId([u8; 32]);

impl SessionId {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        thread_rng().fill_bytes(&mut bytes);
        Self(bytes)
    }
}

#[async_trait]
pub trait Api: Sync + Send {
    /// Begins a user session by verifying the user's username and password.
    ///
    /// # Errors
    ///
    /// May return an `InvalidCredential` error if the username, password, or
    /// both are invalid.
    async fn auth(&self, username: &str, password: &str) -> Result<SessionId, Error>;

    /// Gets the user ID associated with the session token.
    ///
    /// # Errors
    ///
    /// May return an `InvalidToken` error if the token has expired, or is
    /// otherwise invalid.
    async fn user(&self, id: SessionId) -> Result<u128, Error>;
}
