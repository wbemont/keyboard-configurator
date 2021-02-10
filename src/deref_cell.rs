// Wrapper around `OnceCell` implementing `Deref`, and thus also panicking
// when not set (or set twice).
//
// To be used in place of `gtk::TemplateChild`, but without xml.

use once_cell::unsync::OnceCell;

pub struct DerefCell<T>(OnceCell<T>);

impl<T> DerefCell<T> {
    pub fn set(&self, value: T) {
        if self.0.set(value).is_err() {
            panic!("Initialized twice");
        }
    }
}

impl<T> Default for DerefCell<T> {
    fn default() -> Self {
        Self(OnceCell::default())
    }
}

impl <T> std::ops::Deref for DerefCell<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.0.get().unwrap()
    }
}
