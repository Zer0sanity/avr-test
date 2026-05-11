use core::{
    cell::RefCell,
    error::Error,
    fmt,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use avr_device::interrupt::Mutex;

#[derive(Debug)]
pub enum QueueError {
    Full,
    Empty,
    NoItem,
}

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Custom error occurred")
    }
}

impl Error for QueueError {}

pub struct AsyncQueueInner<T, const CAPACITY: usize> {
    count: u8,
    w_idx: u8,
    r_idx: u8,
    buf: [Option<T>; CAPACITY],
    waker: Option<Waker>,
    _phantom: PhantomData<T>,
}

impl<T, const CAPACITY: usize> AsyncQueueInner<T, CAPACITY> {
    const ELEM: Option<T> = const { None };
    const INIT_BUFFER: [Option<T>; CAPACITY] = [Self::ELEM; CAPACITY];

    pub const fn new() -> Self {
        Self {
            count: u8::MIN,
            w_idx: u8::MIN,
            r_idx: u8::MIN,
            buf: Self::INIT_BUFFER,
            waker: None,
            _phantom: PhantomData,
        }
    }

    pub fn has_space(&self) -> bool {
        self.count < (CAPACITY as u8)
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn push(&mut self, item: T) {
        self.buf[self.w_idx as usize] = Some(item);
        self.w_idx = (self.w_idx + 1) % CAPACITY as u8;
        self.count += 1;
    }

    pub fn try_push(&mut self, item: T) -> Result<(), QueueError> {
        match self.has_space() {
            true => Err(QueueError::Full),
            _ => {
                _ = self.waker.take().map(|w| w.wake());
                self.push(item);
                Ok(())
            }
        }
    }

    fn pop(&mut self) -> Option<T> {
        let item = self.buf[self.r_idx as usize].take();
        self.r_idx = (self.r_idx + 1) % CAPACITY as u8;
        self.count -= 1;
        item
    }

    pub fn try_pop(&mut self) -> Result<T, QueueError> {
        match self.is_empty() {
            true => Err(QueueError::Empty),
            _ => {
                _ = self.waker.take().map(|w| w.wake());
                match self.pop() {
                    Some(t) => Ok(t),
                    None => Err(QueueError::NoItem),
                }
            }
        }
    }
}

pub struct AsyncQueue<T, const CAPACITY: usize> {
    inner: Mutex<RefCell<AsyncQueueInner<T, CAPACITY>>>,
}

impl<T, const CAPACITY: usize> AsyncQueue<T, CAPACITY> {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(RefCell::new(AsyncQueueInner::new())),
        }
    }

    pub fn push(&self, item: T) -> PushFuture<'_, T, CAPACITY> {
        PushFuture {
            queue: self,
            item: Some(item),
        }
    }

    pub fn pop(&self) -> PopFuture<'_, T, CAPACITY> {
        PopFuture { queue: self }
    }

    pub fn try_push(&self, item: T) -> Result<(), QueueError> {
        avr_device::interrupt::free(|cs| self.inner.borrow(cs).borrow_mut().try_push(item))
    }

    pub fn try_pop(&self) -> Result<T, QueueError> {
        avr_device::interrupt::free(|cs| self.inner.borrow(cs).borrow_mut().try_pop())
    }
}

pub struct PushFuture<'a, T, const CAPACITY: usize> {
    queue: &'a AsyncQueue<T, CAPACITY>,
    item: Option<T>,
}

impl<'a, T, const CAPACITY: usize> Future for PushFuture<'a, T, CAPACITY> {
    type Output = Result<(), QueueError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        avr_device::interrupt::free(|cs| {
            let mut queue = self.queue.inner.borrow(cs).borrow_mut();

            match queue.has_space() {
                true => {
                    let item = unsafe { self.get_unchecked_mut().item.take() };

                    Poll::Ready(queue.try_push(item.unwrap()))
                }
                _ => {
                    queue.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
    }
}

pub struct PopFuture<'a, T, const CAPACITY: usize> {
    queue: &'a AsyncQueue<T, CAPACITY>,
}

impl<'a, T, const CAPACITY: usize> Future for PopFuture<'a, T, CAPACITY> {
    type Output = Result<T, QueueError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        avr_device::interrupt::free(|cs| {
            let mut queue = self.queue.inner.borrow(cs).borrow_mut();
            match queue.is_empty() {
                true => {
                    queue.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
                _ => Poll::Ready(queue.try_pop()),
            }
        })
    }
}
