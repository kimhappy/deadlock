#[doc(hidden)]
#[macro_export]
macro_rules! __ensure {
    ($condition:expr) => {
        if !$condition {
            return None;
        }
    };
}

pub use __ensure as ensure;
