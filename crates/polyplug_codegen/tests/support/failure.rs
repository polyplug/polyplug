use core::fmt::Display;

pub trait PanicOnFailure<T> {
    #[track_caller]
    fn or_panic(self, message: &str) -> T;
}

impl<T, E> PanicOnFailure<T> for Result<T, E>
where
    E: Display,
{
    fn or_panic(self, message: &str) -> T {
        match self {
            Ok(value) => value,
            Err(error) => panic!("{message}: {error}"),
        }
    }
}

impl<T> PanicOnFailure<T> for Option<T> {
    fn or_panic(self, message: &str) -> T {
        match self {
            Some(value) => value,
            None => panic!("{message}"),
        }
    }
}
