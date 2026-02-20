#[doc(hidden)]
#[macro_export]
macro_rules! __ensure {
    ($condition:expr) => {
        if !$condition {
            return None;
        }
    };

    ($condition:expr, $value:expr) => {
        if !$condition {
            return $value;
        }
    };
}

pub use __ensure as ensure;
