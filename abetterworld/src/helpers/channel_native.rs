pub mod channel {
    use crossbeam_channel::{bounded, Receiver as CbReceiver, Sender as CbSender};

    use crate::helpers::AbwError;

    #[derive(Clone, Debug)]
    pub struct Sender<T> {
        inner: CbSender<T>,
    }

    #[derive(Clone, Debug)]
    pub struct Receiver<T> {
        inner: CbReceiver<T>,
    }

    pub fn channel<T>(bound: usize) -> (Sender<T>, Receiver<T>) {
        let (tx, rx) = bounded(bound);
        (Sender { inner: tx }, Receiver { inner: rx })
    }

    impl<T> Sender<T> {
        pub async fn send(&self, item: T) -> Result<(), ()> {
            self.inner.send(item).map_err(|_| ())
        }
    }

    impl<T> Receiver<T> {
        pub async fn recv(&self) -> Result<T, AbwError> {
            self.inner
                .recv()
                .map_err(|_| AbwError::Paging("Failed to receive item".to_string()))
        }

        pub fn try_recv(&self) -> Result<T, AbwError> {
            self.inner
                .try_recv()
                .map_err(|_| AbwError::Paging("Failed to receive item".to_string()))
        }
    }
}
