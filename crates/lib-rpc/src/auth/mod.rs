use rand::{thread_rng, RngCore};
use tonic::async_trait;

pub mod grpc;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The provided user credentials are not valid.
    #[error("the provided user credentials are invalid")]
    InvalidCredentials,

    #[error("an error occurred during gRPC transport")]
    RpcError(tonic::Code),
}

pub const SESSION_ID_LENGTH: usize = 32;

// 256-bit session token
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SessionId([u8; SESSION_ID_LENGTH]);

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
    async fn authenticate(&self, username: &str, password: &str) -> Result<SessionId, Error>;
}

#[derive(Default)]
pub struct Dummy {}

#[async_trait]
impl Api for Dummy {
    async fn authenticate(&self, _username: &str, _password: &str) -> Result<SessionId, Error> {
        Err(Error::RpcError(tonic::Code::Unavailable))
    }
}
