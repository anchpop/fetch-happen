//! Cooperative cancellation. Dropping a controller does not abort its signals.
use futures::future::{select, Either};
use std::{future::Future, pin::pin};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aborted;
impl std::fmt::Display for Aborted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("operation aborted")
    }
}
impl std::error::Error for Aborted {}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use tokio_util::sync::CancellationToken;

    #[derive(Clone, Debug, Default)]
    pub struct AbortController(CancellationToken);
    #[derive(Clone, Debug)]
    pub struct AbortSignal(CancellationToken);

    impl AbortController {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn abort(&self) {
            self.0.cancel();
        }
        pub fn signal(&self) -> AbortSignal {
            AbortSignal(self.0.clone())
        }
        /// Parent cancellation reaches this controller; cancelling the child never reaches its parent.
        pub fn child_of(signal: &AbortSignal) -> Self {
            Self(signal.0.child_token())
        }
    }
    impl AbortSignal {
        pub fn aborted(&self) -> bool {
            self.0.is_cancelled()
        }
        pub async fn cancelled(&self) {
            self.0.cancelled().await
        }
    }
}
#[cfg(target_arch = "wasm32")]
mod platform {
    use futures::channel::oneshot;
    use wasm_bindgen::{
        convert::{FromWasmAbi, OptionFromWasmAbi},
        describe::WasmDescribe,
    };

    #[derive(Clone, Debug)]
    pub struct AbortSignal(web_sys::AbortSignal);
    #[derive(Clone, Debug)]
    pub struct AbortController(web_sys::AbortController);

    impl AbortController {
        pub fn new() -> Self {
            Self(
                web_sys::AbortController::new()
                    .expect("AbortController is required on this platform"),
            )
        }
        pub fn abort(&self) {
            self.0.abort();
        }
        pub fn signal(&self) -> AbortSignal {
            AbortSignal(self.0.signal())
        }
    }
    impl Default for AbortController {
        fn default() -> Self {
            Self::new()
        }
    }
    impl From<web_sys::AbortSignal> for AbortSignal {
        fn from(signal: web_sys::AbortSignal) -> Self {
            Self(signal)
        }
    }
    impl AbortSignal {
        pub fn as_web(&self) -> &web_sys::AbortSignal {
            &self.0
        }
        pub fn aborted(&self) -> bool {
            self.0.aborted()
        }
        pub async fn cancelled(&self) {
            if self.aborted() {
                return;
            }
            let (sender, receiver) = oneshot::channel();
            // The guard removes the browser listener even when this future is dropped.
            let _listener = gloo_events::EventListener::once(&self.0, "abort", move |_| {
                let _ = sender.send(());
            });
            let _ = receiver.await;
        }
    }
    impl WasmDescribe for AbortSignal {
        fn describe() {
            <web_sys::AbortSignal as WasmDescribe>::describe();
        }
    }
    impl FromWasmAbi for AbortSignal {
        type Abi = <web_sys::AbortSignal as FromWasmAbi>::Abi;
        unsafe fn from_abi(value: Self::Abi) -> Self {
            Self(unsafe { <web_sys::AbortSignal as FromWasmAbi>::from_abi(value) })
        }
    }
    impl OptionFromWasmAbi for AbortSignal {
        fn is_none(value: &Self::Abi) -> bool {
            <web_sys::AbortSignal as OptionFromWasmAbi>::is_none(value)
        }
    }
}
pub use platform::{AbortController, AbortSignal};

impl AbortSignal {
    /// Stop polling and drop the supplied future when aborted. Does not preempt synchronous work.
    pub async fn until<T>(&self, future: impl Future<Output = T>) -> Result<T, Aborted> {
        match select(pin!(self.cancelled()), pin!(future)).await {
            Either::Left(_) => Err(Aborted),
            Either::Right((value, _)) => Ok(value),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use futures::{executor::block_on, FutureExt};
    #[test]
    fn cancellation_is_shared_idempotent_and_one_way() {
        let parent = AbortController::default();
        let child = AbortController::child_of(&parent.signal());
        let sibling = AbortController::child_of(&parent.signal());
        child.abort();
        child.abort();
        assert!(child.signal().aborted());
        assert!(!parent.signal().aborted());
        assert!(!sibling.signal().aborted());
        parent.abort();
        assert!(sibling.signal().aborted());
        assert!(AbortController::child_of(&parent.signal())
            .signal()
            .aborted());
    }
    #[test]
    fn precancellation_skips_work_and_dropping_controller_does_not_cancel() {
        let controller = AbortController::default();
        controller.abort();
        let signal = controller.signal();
        assert_eq!(
            block_on(signal.until(async { panic!("must not run") })),
            Err(Aborted)
        );
        let signal = AbortController::default().signal();
        assert!(signal.cancelled().now_or_never().is_none());
    }
    #[test]
    fn wakes_all_waiters_and_drops_non_send_work() {
        let controller = AbortController::default();
        let signal = controller.signal();
        let owned = std::rc::Rc::new(());
        let work = owned.clone();
        let mut first = Box::pin(signal.until(async move {
            let _guard = work;
            futures::future::pending::<()>().await;
        }));
        assert!(first.as_mut().now_or_never().is_none());
        let mut second = Box::pin(signal.cancelled());
        assert!(second.as_mut().now_or_never().is_none());
        controller.abort();
        assert_eq!(block_on(first), Err(Aborted));
        block_on(second);
        assert_eq!(std::rc::Rc::strong_count(&owned), 1);
    }
}
