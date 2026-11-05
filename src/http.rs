//! GPUI's [`HttpClient`], backed by `reqwest` on the IO runtime.
//!
//! GPUI ships without a working HTTP client. Registering this one lets
//! `img(url)` load remote images (server icons now, avatars later) with GPUI's
//! own caching and loading/fallback states.
//!
//! Requests and responses cross between GPUI's `http` types and reqwest's as
//! plain strings and bytes, so the two crates' `http` versions never have to match.

use std::sync::Arc;

use futures::future::BoxFuture;
use futures::{AsyncReadExt, FutureExt};
use gpui_kit::http_client::http::{HeaderValue, Request, Response};
use gpui_kit::http_client::{AsyncBody, HttpClient, Url};

use crate::runtime::spawn_io;

type Headers = Vec<(String, Vec<u8>)>;

pub struct ReqwestHttpClient {
    client: reqwest::Client,
    user_agent: HeaderValue,
}

impl ReqwestHttpClient {
    pub fn new() -> Arc<Self> {
        let user_agent = concat!("stoat-gpui-poc/", env!("CARGO_PKG_VERSION"));
        Arc::new(Self {
            client: reqwest::Client::builder()
                .user_agent(user_agent)
                .build()
                .expect("failed to build HTTP client"),
            user_agent: HeaderValue::from_static(user_agent),
        })
    }
}

impl HttpClient for ReqwestHttpClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        Some(&self.user_agent)
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn send(
        &self,
        request: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let client = self.client.clone();
        async move {
            let (parts, mut body) = request.into_parts();
            let mut body_bytes = Vec::new();
            body.read_to_end(&mut body_bytes).await?;

            let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())?;
            let url = parts.uri.to_string();
            let headers: Headers = parts
                .headers
                .iter()
                .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
                .collect();

            let (status, headers, bytes) = spawn_io(async move {
                log::debug!("http: {method} {url}");
                let mut request = client.request(method, &url);
                for (name, value) in headers {
                    request = request.header(name, value);
                }
                if !body_bytes.is_empty() {
                    request = request.body(body_bytes);
                }
                let response = request.send().await?;
                let status = response.status().as_u16();
                let headers: Headers = response
                    .headers()
                    .iter()
                    .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
                    .collect();
                let bytes = response.bytes().await?.to_vec();
                anyhow::Ok((status, headers, bytes))
            })
            .await??;

            let mut response = Response::builder().status(status);
            for (name, value) in headers {
                response = response.header(name, value);
            }
            Ok(response.body(AsyncBody::from(bytes))?)
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::ReqwestHttpClient;
    use futures::AsyncReadExt;
    use gpui_kit::http_client::{AsyncBody, HttpClient, http::Request};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn round_trips_a_get_through_reqwest() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/icons/abc"))
            .and(header("accept", "image/*"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(b"png-bytes".to_vec()),
            )
            .expect(1)
            .mount(&server)
            .await;

        let request = Request::get(format!("{}/icons/abc", server.uri()))
            .header("accept", "image/*")
            .body(AsyncBody::empty())
            .unwrap();
        let response = ReqwestHttpClient::new().send(request).await.unwrap();

        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.headers()["content-type"], "image/png");
        let mut body = Vec::new();
        response.into_body().read_to_end(&mut body).await.unwrap();
        assert_eq!(body, b"png-bytes");
    }
}
