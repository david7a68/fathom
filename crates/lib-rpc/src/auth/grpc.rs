mod api {
    tonic::include_proto!("fathom.auth.v0");
}

use tonic::{Request, Response, Status, async_trait};

use super::{Api, Error};

// session-server
#[cfg(any(doc, feature = "auth-server"))]
pub use server::*;

#[cfg(any(doc, feature = "auth-server"))]
mod server {
    use super::*;

    pub use api::auth_server::AuthServer;
    use tonic::transport::NamedService;

    impl<Provider: 'static + Api> AuthServer<Wrapper<Provider>> {
        pub const NAME: &'static str = <Self as NamedService>::NAME;

        pub fn from_provider(provider: Provider) -> Self {
            Self::new(Wrapper(provider))
        }
    }

    pub struct Wrapper<Provider: 'static + Api>(Provider);

    #[async_trait]
    impl<Provider: 'static + Api> api::auth_server::Auth for Wrapper<Provider> {
        async fn authenticate(
            &self,
            request: Request<api::AuthRequest>,
        ) -> Result<Response<api::AuthResponse>, Status> {
            let request = request.into_inner();

            match self
                .0
                .authenticate(&request.username, &request.password)
                .await
            {
                Ok(session_id) => Ok(Response::new(api::AuthResponse {
                    session_id: session_id.0.to_vec(),
                })),
                Err(err) => match err {
                    Error::InvalidCredentials => {
                        Err(Status::unauthenticated("invalid credentials"))
                    }
                    Error::RpcError(status) => Err(Status::new(status, "forwarded")),
                },
            }
        }
    }
}

#[cfg(any(doc, feature = "auth-client"))]
pub use client::*;

#[cfg(any(doc, feature = "auth-client"))]
mod client {
    use super::*;
    use crate::auth::SessionId;
    use tonic::transport::Channel;

    pub struct AuthClient(api::auth_client::AuthClient<Channel>);

    impl AuthClient {
        pub fn new(connection: Channel) -> Self {
            Self(api::auth_client::AuthClient::new(connection))
        }
    }

    #[async_trait]
    impl Api for AuthClient {
        async fn authenticate(&self, username: &str, password: &str) -> Result<SessionId, Error> {
            let r = self
                .0
                .clone()
                .authenticate(api::AuthRequest {
                    username: username.to_owned(),
                    password: password.to_owned(),
                })
                .await;

            match r {
                Ok(response) => {
                    let id = response.into_inner().session_id;
                    Ok(SessionId(id.as_slice().try_into().expect(&format!(
                        "session_id returned from auth service was {} bytes, instead of {} bytes",
                        id.len(),
                        SESSION_ID_LENGTH
                    ))))
                }
                Err(status) => Err(match status.code() {
                    tonic::Code::Unauthenticated => Error::InvalidCredentials,
                    e => Error::RpcError(e),
                }),
            }
        }
    }
}
