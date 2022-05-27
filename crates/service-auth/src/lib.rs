use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use async_trait::async_trait;
use lib_rpc::auth::{Api, Error, SessionId};

pub struct AuthService {
    identities: HashMap<String, String>,
    sessions: Mutex<HashSet<SessionId>>,
}

impl AuthService {
    pub fn new() -> Self {
        Self {
            identities: HashMap::new(),
            sessions: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait]
impl Api for AuthService {
    async fn authenticate(&self, username: &str, password: &str) -> Result<SessionId, Error> {
        if let Some(expected_password) = self.identities.get(username) {
            if expected_password == password {
                let id = SessionId::generate();
                self.sessions.lock().unwrap().insert(id.clone());
                return Ok(id);
            }
        }

        Err(Error::InvalidCredentials)
    }
}
