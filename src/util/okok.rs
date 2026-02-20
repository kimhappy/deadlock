pub trait Okok {
    type Value;

    fn okok(self) -> Option<Self::Value>;
    unsafe fn okok_unchecked(self) -> Self::Value;
}

impl<'a, T, E> Okok for &'a Result<T, E> {
    type Value = &'a T;

    fn okok(self) -> Option<Self::Value> {
        self.as_ref().ok()
    }

    unsafe fn okok_unchecked(self) -> Self::Value {
        unsafe { self.as_ref().unwrap_unchecked() }
    }
}

impl<'a, T, E> Okok for &'a mut Result<T, E> {
    type Value = &'a mut T;

    fn okok(self) -> Option<Self::Value> {
        self.as_mut().ok()
    }

    unsafe fn okok_unchecked(self) -> Self::Value {
        unsafe { self.as_mut().unwrap_unchecked() }
    }
}

impl<T, E> Okok for Result<T, E> {
    type Value = T;

    fn okok(self) -> Option<Self::Value> {
        self.ok()
    }

    unsafe fn okok_unchecked(self) -> Self::Value {
        unsafe { self.unwrap_unchecked() }
    }
}
