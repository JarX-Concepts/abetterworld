// platform_await.rs
use std::future::Future;

#[cfg(not(target_arch = "wasm32"))]
pub trait PlatformAwait {
    type Output;
    fn platform_await(self) -> Self::Output;
}

#[cfg(target_arch = "wasm32")]
pub trait PlatformAwait {
    type Output;
    type Fut: Future<Output = Self::Output>;
    fn platform_await(self) -> Self::Fut;
}

#[cfg(not(target_arch = "wasm32"))]
impl<Fut> PlatformAwait for Fut
where
    Fut: Future,
{
    type Output = Fut::Output;

    fn platform_await(self) -> Self::Output {
        futures::executor::block_on(self)
    }
}

#[cfg(target_arch = "wasm32")]
impl<Fut> PlatformAwait for Fut
where
    Fut: Future,
{
    type Output = Fut::Output;
    type Fut = Fut;

    fn platform_await(self) -> Self::Fut {
        self // no-op passthrough
    }
}
