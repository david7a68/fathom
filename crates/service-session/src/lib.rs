//! Browser session API.

use async_trait::async_trait;
use base_comm::session::{Api, Error, SessionId};

pub struct Session {}

impl Session {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl Api for Session {
    async fn auth(&self, _username: &str, _password: &str) -> Result<SessionId, Error> {
        Err(Error::InvalidCredential)
    }

    async fn user(&self, _token: SessionId) -> Result<u128, Error> {
        Err(Error::InvalidToken)
    }
}
