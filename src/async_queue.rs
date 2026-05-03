use core::{
    cell::RefCell,
    marker::PhantomData,
    mem::MaybeUninit,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use avr_device::interrupt::Mutex;

pub struct AsyncQueueInner<T, const CAPACITY: usize, const WAKERS: usize> {
    length: usize,
    write_index: usize,
    buffer: [MaybeUninit<T>; CAPACITY],
    wakers: [Option<Waker>; WAKERS],
    _phantom: PhantomData<T>,
}

impl<T, const CAPACITY: usize, const WAKERS: usize> AsyncQueueInner<T, CAPACITY, WAKERS> {
    const ELEM: MaybeUninit<T> = MaybeUninit::uninit();
    const INIT_BUFFER: [MaybeUninit<T>; CAPACITY] = [Self::ELEM; CAPACITY];
    const INIT_WAKERS: [Option<Waker>; WAKERS] = [const { None }; WAKERS];

    pub const fn new() -> Self {
        Self {
            length: usize::MIN,
            write_index: usize::MIN,
            buffer: Self::INIT_BUFFER,
            wakers: Self::INIT_WAKERS,
            _phantom: PhantomData,
        }
    }

    pub fn has_space(&self) -> bool {
        self.length < CAPACITY
    }

    pub fn push(&mut self, item: T) {
        self.buffer[self.write_index].write(item);
        self.write_index = (self.write_index + 1) % CAPACITY;
        self.length += 1;
    }
}

pub struct AsyncQueue<T, const CAPACITY: usize, const WAKERS: usize> {
    inner: Mutex<RefCell<AsyncQueueInner<T, CAPACITY, WAKERS>>>,
}

impl<T, const CAPACITY: usize, const WAKERS: usize> AsyncQueue<T, CAPACITY, WAKERS> {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(RefCell::new(AsyncQueueInner::new())),
        }
    }

    pub fn push(&self, item: T) -> PushFuture<'_, T, CAPACITY, WAKERS> {
        PushFuture {
            queue: self,
            item: Some(item),
        }
    }
}

pub struct PushFuture<'a, T, const CAPACITY: usize, const WAKERS: usize> {
    queue: &'a AsyncQueue<T, CAPACITY, WAKERS>,
    item: Option<T>,
}

impl<'a, T, const CAPACITY: usize, const WAKERS: usize> Future
    for PushFuture<'a, T, CAPACITY, WAKERS>
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        avr_device::interrupt::free(|cs| {
            let mut queue = self.queue.inner.borrow(cs).borrow_mut();

            match queue.has_space() {
                true => {
                    let this = unsafe { self.get_unchecked_mut() };
                    let item = this.item.take().expect("future polled after completion");
                    queue.push(item);
                    Poll::Ready(())
                }
                _ => {
                    *queue
                        .wakers
                        .iter_mut()
                        .find(|slot| slot.is_none())
                        .expect("Waiters list full") = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
    }
}
