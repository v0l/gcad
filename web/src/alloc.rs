use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};

const SIZES: [usize; 40] = [
    16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896, 1024,
    1280, 1536, 1792, 2048, 2560, 3072, 3584, 4096, 5120, 6144, 7168, 8192, 10240, 12288, 14336,
    16384, 20480, 24576, 28672, 32768,
];
const CLASSES: usize = SIZES.len();
const ALIGN: usize = 16;
const CHUNK: usize = 64 * 1024;
const KEEP: usize = 256 * 1024;

fn class_of(layout: Layout) -> Option<usize> {
    if layout.align() > ALIGN || layout.size() > SIZES[CLASSES - 1] {
        return None;
    }
    let size = layout.size().max(1);
    Some(SIZES.partition_point(|&s| s < size))
}

fn batch(class: usize) -> usize {
    (CHUNK / SIZES[class]).max(2)
}

fn keep(class: usize) -> usize {
    (KEEP / SIZES[class]).max(4)
}

#[derive(Clone, Copy)]
struct List {
    head: *mut u8,
    len: usize,
}

const EMPTY: List = List {
    head: null_mut(),
    len: 0,
};

unsafe fn next(block: *mut u8) -> *mut u8 {
    unsafe { *(block as *mut *mut u8) }
}

unsafe fn link(block: *mut u8, to: *mut u8) {
    unsafe { *(block as *mut *mut u8) = to }
}

impl List {
    unsafe fn push(&mut self, block: *mut u8) {
        unsafe { link(block, self.head) };
        self.head = block;
        self.len += 1;
    }

    unsafe fn pop(&mut self) -> *mut u8 {
        let block = self.head;
        if !block.is_null() {
            self.head = unsafe { next(block) };
            self.len -= 1;
        }
        block
    }

    unsafe fn take(&mut self, count: usize) -> List {
        let count = count.min(self.len);
        if count == 0 {
            return EMPTY;
        }
        let head = self.head;
        let mut tail = head;
        for _ in 1..count {
            tail = unsafe { next(tail) };
        }
        self.head = unsafe { next(tail) };
        self.len -= count;
        unsafe { link(tail, null_mut()) };
        List { head, len: count }
    }

    unsafe fn append(&mut self, other: List) {
        if other.len == 0 {
            return;
        }
        let mut tail = other.head;
        while !unsafe { next(tail) }.is_null() {
            tail = unsafe { next(tail) };
        }
        unsafe { link(tail, self.head) };
        self.head = other.head;
        self.len += other.len;
    }
}

struct Shared {
    busy: AtomicBool,
    lists: UnsafeCell<[List; CLASSES]>,
}

unsafe impl Sync for Shared {}

static SHARED: Shared = Shared {
    busy: AtomicBool::new(false),
    lists: UnsafeCell::new([EMPTY; CLASSES]),
};

fn with_shared<R>(f: impl FnOnce(&mut [List; CLASSES]) -> R) -> R {
    while SHARED
        .busy
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::hint::spin_loop();
    }
    let out = f(unsafe { &mut *SHARED.lists.get() });
    SHARED.busy.store(false, Ordering::Release);
    out
}

thread_local! {
    static LOCAL: UnsafeCell<[List; CLASSES]> = const { UnsafeCell::new([EMPTY; CLASSES]) };
}

unsafe fn carve(class: usize) -> List {
    let size = SIZES[class];
    let count = batch(class);
    let chunk = unsafe { System.alloc(Layout::from_size_align_unchecked(size * count, ALIGN)) };
    let mut list = EMPTY;
    if chunk.is_null() {
        return list;
    }
    for i in (0..count).rev() {
        unsafe { list.push(chunk.add(i * size)) };
    }
    list
}

pub struct Cached;

unsafe impl GlobalAlloc for Cached {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let Some(class) = class_of(layout) else {
            return unsafe { System.alloc(layout) };
        };
        let got = LOCAL.try_with(|cell| {
            let local = unsafe { &mut (*cell.get())[class] };
            if local.len == 0 {
                let refill = with_shared(|shared| unsafe { shared[class].take(batch(class)) });
                let refill = if refill.len == 0 {
                    unsafe { carve(class) }
                } else {
                    refill
                };
                unsafe { local.append(refill) };
            }
            unsafe { local.pop() }
        });
        match got {
            Ok(block) => block,
            Err(_) => unsafe {
                System.alloc(Layout::from_size_align_unchecked(SIZES[class], ALIGN))
            },
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let Some(class) = class_of(layout) else {
            return unsafe { System.dealloc(ptr, layout) };
        };
        let kept = LOCAL.try_with(|cell| {
            let local = unsafe { &mut (*cell.get())[class] };
            unsafe { local.push(ptr) };
            if local.len > keep(class) {
                let spare = unsafe { local.take(local.len / 2) };
                with_shared(|shared| unsafe { shared[class].append(spare) });
            }
        });
        if kept.is_err() {
            with_shared(|shared| unsafe { shared[class].push(ptr) });
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        match (class_of(layout), class_of(new_layout)) {
            (Some(a), Some(b)) if a == b => ptr,
            (None, None) => unsafe { System.realloc(ptr, layout, new_size) },
            _ => {
                let fresh = unsafe { self.alloc(new_layout) };
                if !fresh.is_null() {
                    unsafe {
                        std::ptr::copy_nonoverlapping(ptr, fresh, layout.size().min(new_size));
                        self.dealloc(ptr, layout);
                    }
                }
                fresh
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_are_aligned_and_distinct_across_threads() {
        let threads: Vec<_> = (0..8)
            .map(|t| {
                std::thread::spawn(move || {
                    let mut held: Vec<(usize, Layout)> = Vec::new();
                    for round in 0..20_000usize {
                        let size = 1 + (round * 37 + t * 101) % 40_000;
                        let layout = Layout::from_size_align(size, 8).unwrap();
                        let block = unsafe { Cached.alloc(layout) };
                        assert!(!block.is_null());
                        assert_eq!(block as usize % 8, 0);
                        unsafe { block.write_bytes((round % 251) as u8, size) };
                        held.push((block as usize, layout));
                        if round % 3 == 0 {
                            let (block, layout) = held.swap_remove(round % held.len());
                            unsafe { Cached.dealloc(block as *mut u8, layout) };
                        }
                    }
                    held
                })
            })
            .collect();
        let mut spans: Vec<(usize, usize)> = threads
            .into_iter()
            .flat_map(|t| t.join().unwrap())
            .map(|(block, layout)| (block, block + layout.size()))
            .collect();
        spans.sort();
        for pair in spans.windows(2) {
            assert!(pair[0].1 <= pair[1].0, "{pair:?} overlap");
        }
    }

    #[test]
    fn realloc_keeps_contents() {
        let layout = Layout::from_size_align(24, 8).unwrap();
        let block = unsafe { Cached.alloc(layout) };
        unsafe { block.copy_from(b"abcdefghijklmnopqrstuvwx".as_ptr(), 24) };
        let same = unsafe { Cached.realloc(block, layout, 30) };
        assert_eq!(same, block);
        let grown = unsafe { Cached.realloc(same, Layout::from_size_align(30, 8).unwrap(), 5000) };
        let text = unsafe { std::slice::from_raw_parts(grown, 24) };
        assert_eq!(text, b"abcdefghijklmnopqrstuvwx");
        unsafe { Cached.dealloc(grown, Layout::from_size_align(5000, 8).unwrap()) };
    }
}
