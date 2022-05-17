use axum::{
    http::{StatusCode},
    response::IntoResponse,
    routing::post,
    Extension, Json, Router,
};
use axum_extra::extract::cookie::{Cookie, Key, PrivateCookieJar};
use comm::session;
use serde::Deserialize;

use std::sync::Arc;

pub struct RestApi {
    session: Arc<dyn session::Api>,
}

impl RestApi {
    pub fn new(session: Arc<dyn session::Api>) -> Self {
        Self { session }
    }

    pub fn routes(self: Arc<Self>) -> Router {
        Router::new().merge(self.auth())
    }

    fn auth(self: Arc<Self>) -> Router {
        Router::new()
            .route("/auth", post(authenticate_session).get(get_session_user))
            .layer(Extension(self))
            .layer(Extension(Key::generate()))
    }
}

#[derive(Deserialize)]
struct UserCredentials {
    username: String,
    password: String,
}

async fn authenticate_session(
    Extension(api): Extension<Arc<RestApi>>,
    Json(creds): Json<UserCredentials>,
    jar: PrivateCookieJar,
) -> impl IntoResponse {
    if let Ok(token) = api.session.auth(&creds.username, &creds.password).await {
        Ok(jar.add(Cookie::new("session_key", format!("{}", token))))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn get_session_user(
    Extension(api): Extension<Arc<RestApi>>,
    jar: PrivateCookieJar,
) -> impl IntoResponse {
    if let Some(token) = jar.get("session_key") {
        let session_id = decode_u128(token.value()).ok_or(StatusCode::NOT_ACCEPTABLE)?;

        if let Ok(user) = api.session.user(session_id).await {
            Ok(encode_u128(user))
        } else {
            Err(StatusCode::UNAUTHORIZED)
        }
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn encode_u128(id: u128) -> String {
    format!("{:X}", id)
}

fn decode_u128(s: &str) -> Option<u128> {
    u128::from_str_radix(s, 16).ok()
}
