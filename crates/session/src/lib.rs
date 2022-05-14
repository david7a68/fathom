//! Browser session API.

use comm::session::{Api, Error, Token};

pub struct Session {}

impl Session {
    pub fn new() -> Self {
        Self {}
    }
}

impl Api for Session {
    fn auth(&self, _username: &str, _password_hash: u128) -> Result<Token, Error> {
        Err(Error::InvalidCredential)
    }

    fn user(&self, _token: Token) -> Result<u128, Error> {
        Err(Error::InvalidToken)
    }
}
