use crate::helpers::AbwError;

use bytes::Bytes;
use reqwest::header::HeaderMap;
use reqwest::{Client as InnerClient, RequestBuilder};
use reqwest::{Error, Response as InnerResponse};

#[derive(Debug, Clone)]
pub struct Client {
    inner: InnerClient,
}

#[derive(Debug)]
pub struct Request {
    inner: RequestBuilder,
}

#[derive(Debug)]
pub struct Response {
    inner: InnerResponse,
}

impl Client {
    pub fn new(_threads: usize) -> Result<Client, AbwError> {
        let client = InnerClient::builder()
            .user_agent("abetterworld")
            .build()
            .map_err(|e| AbwError::Network(format!("Failed to build HTTP client: {e}")))?;
        Ok(Self { inner: client })
    }

    pub fn get(&self, url: &str) -> Request {
        Request {
            inner: self.inner.get(url),
        }
    }
}

impl Request {
    pub fn query<T: serde::Serialize + ?Sized>(mut self, query: &T) -> Self {
        self.inner = self.inner.query(query);
        self
    }

    pub async fn send(self) -> Result<Response, Error> {
        let inner = self.inner.send().await?;
        Ok(Response { inner })
    }
}

impl Response {
    pub async fn bytes(self) -> Result<Bytes, Error> {
        self.inner.bytes().await
    }

    pub fn headers(&self) -> &HeaderMap {
        self.inner.headers()
    }
}
