use axum::{
    extract::{WebSocketUpgrade, ws::Message}, http::StatusCode, response::IntoResponse, routing::post, Extension,
    Json, Router,
};
use axum_extra::extract::{PrivateCookieJar, cookie::{Cookie, Key}};
use base_comm::session;
use serde::Deserialize;
use serde_json::json;

use std::sync::Arc;

pub struct RestApi {
    key: Key,
    session: Arc<dyn session::Api>,
}

impl RestApi {
    pub fn new(session: Arc<dyn session::Api>) -> Self {
        Self { key: Key::generate(), session }
    }

    pub fn routes(self: Arc<Self>) -> Router {
        Router::new()
            .route("/api", post(authenticate_session).get(connect_socket))
            .layer(Extension(self.key.clone()))
            .layer(Extension(self))
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
) -> Result<PrivateCookieJar, StatusCode> {
    if let Ok(token) = api.session.auth(&creds.username, &creds.password).await {
        // how to encode 256-bit session token into a string?
        // maybe just use hex for now? or maybe base64?
        Ok(jar.add(Cookie::new("id", format!("{}", 0))))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn connect_socket(
    Extension(api): Extension<Arc<RestApi>>,
    ws: WebSocketUpgrade,
    jar: PrivateCookieJar,
) -> impl IntoResponse {
    if let Some(id) = jar.get("id") {
        // how to decode string into 256-bit session token?
        // test that id is a valid session token
        // if it is, continue with socket connection
        // else return unauthorized
    }

    let _api = api.clone();
    ws.on_upgrade(|mut socket| async move {
        while let Some(msg) = socket.recv().await {
            let msg = if let Ok(msg) = msg {
                msg
            } else {
                return;
            };

            match msg {
                Message::Text(_) => todo!(),
                Message::Binary(_) => todo!(),
                Message::Close(_) => todo!(),
                _ => {}
            }

            // if socket.send(msg).await.is_err() {
            //     return;
            // }
        }
    });
}
