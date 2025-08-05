use crate::helpers::AbwError;
use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use reqwest::{Client as InnerClient, RequestBuilder};
        use reqwest::{Response as InnerResponse, Error};
        use reqwest::header::{HeaderMap};
        use bytes::Bytes;

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
            pub fn new(_threads: usize) -> Result<Client, AbwError>  {
                let client = InnerClient::builder()
                            .user_agent("abetterworld")
                            .build()
                            .map_err(|e| AbwError::Network(format!("Failed to build HTTP client: {e}")))?;
                Ok(Self {
                    inner: client
                })
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
    } else {
        use reqwest::blocking::{Client as InnerClient, RequestBuilder};
        use reqwest::blocking::{Response as InnerResponse};
        use reqwest::{Error};
        use reqwest::header::{HeaderName, HeaderValue, HeaderMap};
        use bytes::Bytes;
        use std::future::{ready, Ready};

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
            pub fn new(threads: usize) -> Result<Client, AbwError>  {
                let client = InnerClient::builder()
                            .user_agent("abetterworld")
                            .pool_max_idle_per_host(threads + 1)
                            .build()
                            .map_err(|e| AbwError::Network(format!("Failed to build HTTP client: {e}")))?;
                Ok(Self {
                    inner: client
                })
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

            pub fn header(
                mut self,
                key: impl Into<HeaderName>,
                value: impl Into<HeaderValue>,
            ) -> Self {
                self.inner = self.inner.header(key, value);
                self
            }

            pub fn send(self) -> Ready<Result<Response, Error>> {
                ready(self.inner.send().map(|inner| Response { inner }))
            }
        }

        impl Response {
            pub async fn bytes(self) -> Result<Bytes, Error> {
                Ok(self.inner.bytes()?)
            }

            pub fn headers(&self) -> &HeaderMap {
                self.inner.headers()
            }
        }
    }
}
