pub mod channel {
    use crate::helpers::AbwError;
    use futures::channel::mpsc::{channel as fchannel, Receiver as FReceiver, Sender as FSender};
    use futures::SinkExt;
    use futures::StreamExt;

    #[derive(Clone, Debug)]
    pub struct Sender<T> {
        inner: FSender<T>,
    }

    pub struct Receiver<T> {
        inner: FReceiver<T>,
    }

    pub fn channel<T>(bound: usize) -> (Sender<T>, Receiver<T>) {
        let (tx, rx) = fchannel(bound);
        (Sender { inner: tx }, Receiver { inner: rx })
    }

    impl<T> Sender<T> {
        pub async fn send(&mut self, item: T) -> Result<(), AbwError> {
            self.inner
                .send(item)
                .await
                .map_err(|_| AbwError::Paging("Failed to send item".to_string()))
        }

        pub fn try_send(&mut self, item: T) -> Result<(), AbwError> {
            self.inner
                .try_send(item)
                .map_err(|_| AbwError::Paging("Failed to send item".to_string()))
        }
    }

    impl<T> Receiver<T> {
        pub async fn recv(&mut self) -> Result<T, ()> {
            self.inner.next().await.ok_or(())
        }

        pub fn try_recv(&mut self) -> Result<T, AbwError> {
            self.inner
                .try_next()
                .map_err(|_| AbwError::Paging("Failed to receive item".to_string()))?
                .ok_or(AbwError::Paging("Channel closed".to_string()))
        }

        // Optional non-blocking poll
        pub fn poll_next(&mut self) -> Option<T> {
            self.inner.try_next().ok().flatten()
        }
    }
}
