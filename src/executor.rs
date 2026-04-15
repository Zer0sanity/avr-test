use core::{
    pin::Pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

// A simple helper to run two tasks concurrently
pub struct Join<A, B> {
    pub a: A,
    pub b: B,
}

impl<A: Future<Output = ()>, B: Future<Output = ()>> Future for Join<A, B> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Safety: We are manually projecting pinning to the inner fields
        let (a, b) = unsafe {
            let this = self.get_unchecked_mut();
            (
                Pin::new_unchecked(&mut this.a),
                Pin::new_unchecked(&mut this.b),
            )
        };

        // Poll both tasks. We don't care about the return Poll here
        // because these tasks loop forever.
        let _ = a.poll(cx);
        let _ = b.poll(cx);

        Poll::Pending
    }
}

fn dummy_raw_waker() -> RawWaker {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        dummy_raw_waker()
    }

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
    RawWaker::new(core::ptr::null(), &VTABLE)
}

pub struct Executor<F: Future> {
    future: F,
}

impl<F: Future> Executor<F> {
    pub fn new(future: F) -> Self {
        Self { future }
    }

    pub fn run(&mut self) {
        let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
        let mut cx = Context::from_waker(&waker);

        // Pin the future to the stack
        let mut pinned_future = unsafe { Pin::new_unchecked(&mut self.future) };

        loop {
            match pinned_future.as_mut().poll(&mut cx) {
                Poll::Ready(_) => break, // Task finished
                Poll::Pending => {
                    // Optional: Put the CPU to sleep here to save power
                    // avr_device::asm::sleep();
                }
            }
        }
    }
}
