pub use imp::thread_local as imp;

pub mod traits {
    pub use super::{Key as sys_Key, OsKey as sys_OsKey, StaticOsKey as sys_StaticOsKey};
}

pub mod prelude {
    pub use super::imp::{Key, OsKey, StaticOsKey};
    pub use super::traits::*;
}

use core::cell::UnsafeCell;

pub trait Key<T> {
    // const fn new() -> Self;
    unsafe fn get(&'static self) -> Option<&'static UnsafeCell<Option<T>>>;
}

pub trait StaticOsKey {
    // const fn new(dtor: Option<unsafe extern fn(*mut u8)>) -> Self;

    unsafe fn get(&self) -> *mut u8;
    unsafe fn set(&self, val: *mut u8);
    unsafe fn destroy(&self);
}

pub trait OsKey {
    fn new(dtor: Option<unsafe extern fn(*mut u8)>) -> Self;

    fn get(&self) -> *mut u8;
    fn set(&self, val: *mut u8);
}

pub mod os {
    use core::cell::{Cell, UnsafeCell};
    use alloc::boxed::Box;
    use core::marker;
    use core::ptr;
    use super::prelude::*;

    pub struct Key<T> {
        // OS-TLS key that we'll use to key off.
        os: StaticOsKey,
        marker: marker::PhantomData<Cell<T>>,
    }

    unsafe impl<T> marker::Sync for Key<T> { }

    struct Value<T: 'static> {
        key: &'static Key<T>,
        value: UnsafeCell<Option<T>>,
    }

    impl<T: 'static> Key<T> {
        const fn new() -> Self {
            Key {
                os: StaticOsKey::new(Some(destroy_value::<T>)),
                marker: marker::PhantomData
            }
        }
    }

    impl<T: 'static> super::Key<T> for Key<T> {
        unsafe fn get(&'static self) -> Option<&'static UnsafeCell<Option<T>>> {
            let ptr = StaticOsKey::get(&self.os) as *mut Value<T>;
            if !ptr.is_null() {
                if ptr as usize == 1 {
                    return None
                }
                return Some(&(*ptr).value);
            }

            // If the lookup returned null, we haven't initialized our own local
            // copy, so do that now.
            let ptr: Box<Value<T>> = Box::new(Value {
                key: self,
                value: UnsafeCell::new(None),
            });
            let ptr = Box::into_raw(ptr);
            StaticOsKey::set(&self.os, ptr as *mut u8);
            Some(&(*ptr).value)
        }
    }

    pub unsafe extern fn destroy_value<T: 'static>(ptr: *mut u8) {
        // The OS TLS ensures that this key contains a NULL value when this
        // destructor starts to run. We set it back to a sentinel value of 1 to
        // ensure that any future calls to `get` for this thread will return
        // `None`.
        //
        // Note that to prevent an infinite loop we reset it back to null right
        // before we return from the destructor ourselves.
        let ptr = Box::from_raw(ptr as *mut Value<T>);
        let key = ptr.key;
        StaticOsKey::set(&key.os, 1 as *mut u8);
        drop(ptr);
        StaticOsKey::set(&key.os, ptr::null_mut());
    }
}
