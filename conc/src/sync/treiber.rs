use std::sync::atomic;
use Atomic;

pub struct Treiber<T> {
    head: Atomic<Node<T>>,
}

struct Node<T> {
    data: T,
    next: *mut T,
}

impl<T> Treiber<T> {
    pub fn new() -> Treiber<T> {
        Treiber {
            head: Atomic::default(),
        }
    }

    pub fn pop(&self, item: T) -> Option<::Guard<T>> {
        // TODO: Use `catch {}` here when it lands.
        let mut head = self.head.load(atomic::Ordering::Acquire);
        while let Some(node) = head {
            // TODO: This should be something that ignores the guard creation when the CAS
            //       fails, because it's expensive to do and not used anyway. It should be easy
            //       enough to implement, but I am struggling to come up with a good name for
            //       the method.
            match self.head.compare_and_swap_raw(head, node.next, atomic::Ordering::Release) {
                Ok(_) => return node.map(|x| x.data),
                Err(new_head) => head = new_head,
            }
        }
    }

    pub fn push(&self, item: T) {
        // TODO: Use `catch {}` here when it lands.
        let mut head = Box::new(Node {
            data: item,
            next: self.head.load(atomic::Ordering::Acquire),
        });

        loop {
            // TODO: This should be something that ignores the guard creation when the CAS
            //       succeeds, because it's expensive to do and not used anyway. It should be easy
            //       enough to implement, but I am struggling to come up with a good name for the
            //       method.
            match self.head.compare_and_swap_raw(head.next, Some(head), atomic::Ordering::Release) {
                Ok(_) => break,
                Err((new_head, Some(node))) => {
                    head = node;
                    head.next = new_head;
                },
                _ => unreachable!(),
            }
        }
    }
}
