use core::fmt::Debug;

pub trait PanicOnSuccess<E> {
    #[track_caller]
    fn err_or_panic(self, message: &str) -> E;
}

impl<T, E> PanicOnSuccess<E> for Result<T, E>
where
    T: Debug,
{
    fn err_or_panic(self, message: &str) -> E {
        match self {
            Ok(value) => panic!("{message}: {value:?}"),
            Err(error) => error,
        }
    }
}
