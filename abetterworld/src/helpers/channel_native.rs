pub mod channel {
    use crossbeam_channel::{bounded, Receiver as CbReceiver, Sender as CbSender};

    pub struct Sender<T> {
        inner: CbSender<T>,
    }

    pub struct Receiver<T> {
        inner: CbReceiver<T>,
    }

    pub fn channel<T>(bound: usize) -> (Sender<T>, Receiver<T>) {
        let (tx, rx) = bounded(bound);
        (Sender { inner: tx }, Receiver { inner: rx })
    }

    impl<T> Sender<T> {
        pub fn send(&self, item: T) -> Result<(), ()> {
            self.inner.send(item).map_err(|_| ())
        }
    }

    impl<T> Receiver<T> {
        pub fn recv(&self) -> Result<T, ()> {
            self.inner.recv().map_err(|_| ())
        }

        // Optional non-blocking poll
        pub fn try_recv(&self) -> Result<T, ()> {
            self.inner.try_recv().map_err(|_| ())
        }
    }
}
