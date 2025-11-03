pub mod channel {
    use crate::helpers::AbwError;
    use async_channel::{bounded, unbounded, Receiver as AReceiver, Sender as ASender};

    #[derive(Clone, Debug)]
    pub struct Sender<T> {
        inner: ASender<T>,
    }

    #[derive(Clone, Debug)]
    pub struct Receiver<T> {
        inner: AReceiver<T>,
    }

    pub fn channel<T>(bound: usize) -> (Sender<T>, Receiver<T>) {
        let (tx, rx) = if bound == 0 {
            unbounded()
        } else {
            bounded(bound)
        };
        (Sender { inner: tx }, Receiver { inner: rx })
    }

    impl<T> Sender<T> {
        pub async fn send(&self, item: T) -> Result<(), AbwError> {
            self.inner
                .send(item)
                .await
                .map_err(|_| AbwError::Paging("Failed to send item".to_string()))
        }

        pub fn try_send(&self, item: T) -> Result<(), AbwError> {
            self.inner
                .try_send(item)
                .map_err(|_| AbwError::Paging("Failed to send item".to_string()))
        }
    }

    impl<T> Receiver<T> {
        pub async fn recv(&self) -> Result<T, AbwError> {
            self.inner
                .recv()
                .await
                .map_err(|_| AbwError::Paging("Channel closed".to_string()))
        }

        pub fn try_recv(&self) -> Result<T, AbwError> {
            self.inner
                .try_recv()
                .map_err(|_| AbwError::Paging("Channel empty/closed".to_string()))
        }

        pub fn poll_next(&self) -> Option<T> {
            self.inner.try_recv().ok()
        }
    }
}
