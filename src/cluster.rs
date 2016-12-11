use std::NonZero;

pub struct ClusterPointer(NonZero<u64>);

impl ClusterPointer {
    pub fn new(x: u64) -> Option<ClusterPointer> {
        if x == 0 {
            None
        } else {
            Some(ClusterPointer(unsafe {
                NonZero::new(x)
            }))
        }
    }
}
