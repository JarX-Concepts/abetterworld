#[cfg(feature = "profile-tracy")]
pub fn init_profiling() {
    use tracing_subscriber::{layer::SubscriberExt, Registry};
    let tracy_layer = tracing_tracy::TracyLayer::default();
    let subscriber = Registry::default().with(tracy_layer);
    tracing::subscriber::set_global_default(subscriber).unwrap();

    // Optional niceties from tracy-client:
    tracy_client::set_thread_name!("abw-main");
    // mark the first frame boundary early so you see a timeline immediately:
    tracy_client::frame_mark();
}

#[cfg(not(feature = "profile-tracy"))]
pub fn init_profiling() {}

#[macro_export]
macro_rules! set_thread_name {
    ($name:expr) => {
        #[cfg(feature = "profile-tracy")]
        {
            tracy_client::set_thread_name!($name);

            tracy_client::frame_mark();
        }
    };
}
