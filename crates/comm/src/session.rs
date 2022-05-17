use async_trait::async_trait;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The provided session token has either expired, or is otherwise invalid.
    #[error("the provided token is invalid, it may have expired")]
    InvalidToken,

    /// The provided user credentials are not valid.
    #[error("the provided user credentials are invalid")]
    InvalidCredential,
}

pub type Token = u128;

#[async_trait]
pub trait Api: Sync + Send {
    /// Begins a user session by verifying the user's username and password.
    ///
    /// # Errors
    ///
    /// May return an `InvalidCredential` error if the username, password, or
    /// both are invalid.
    async fn auth(&self, username: &str, password: &str) -> Result<Token, Error>;

    /// Gets the user ID associated with the session token.
    ///
    /// # Errors
    ///
    /// May return an `InvalidToken` error if the token has expired, or is
    /// otherwise invalid.
    async fn user(&self, token: Token) -> Result<u128, Error>;
}
