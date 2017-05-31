pub struct Treiber;

/*
pub struct Treiber<T> {
    head: ::Option<Node<T>>,
}

struct Node<T> {
    data: T,
    next: ::Option<Node<T>>,
}

impl<T> Treiber<T> {
    pub fn new() -> Stack<T> {
        Stack {
            head: ::Option::default(),
        }
    }

    pub fn push(&self, item: T) -> Option<T> {
        // TODO: Use `catch {}` here when it lands.
        let mut head = self.head.load(atomic::Ordering::Acquire);
        loop {
            if let Some(head) =  {
                let next = head.next

                match self.head.compare_and_swap(head, next, atomic::Ordering::Release) {
                    Ok()
                    Err(new_head) => head = new_head,
                }
            } else {
                return None;
            }
        }
    }
}
*/
