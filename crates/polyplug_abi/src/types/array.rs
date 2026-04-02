#[repr(C)]
pub struct Array<T> where T: Sized {
    pub items: *const T,
    pub size: usize,
}
