pub trait Okok {
    type Value;

    unsafe fn okok_unchecked(self) -> Self::Value;
}

impl<'a, T, E> Okok for &'a Result<T, E> {
    type Value = &'a T;

    unsafe fn okok_unchecked(self) -> Self::Value {
        unsafe { self.as_ref().unwrap_unchecked() }
    }
}

impl<'a, T, E> Okok for &'a mut Result<T, E> {
    type Value = &'a mut T;

    unsafe fn okok_unchecked(self) -> Self::Value {
        unsafe { self.as_mut().unwrap_unchecked() }
    }
}

impl<T, E> Okok for Result<T, E> {
    type Value = T;

    unsafe fn okok_unchecked(self) -> Self::Value {
        unsafe { self.unwrap_unchecked() }
    }
}
