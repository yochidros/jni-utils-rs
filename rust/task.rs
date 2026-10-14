use ::jni::{
    errors::Result,
    objects::{JMethodID, JObject},
    signature::ReturnType,
    JNIEnv,
};
use std::task::Waker;

/// Wraps the given waker in a `io.github.gedgygedgy.rust.task.Waker` object.
///
/// Calling this function is generally not necessary, since
/// [`JFuture`](crate::future::JFuture) and [`JStream`](crate::stream::JStream)
/// take care of it for you.
///
/// # Arguments
///
/// * `env` - Java environment in which to create the object.
/// * `waker` - Waker to wrap in a Java object.
pub fn waker<'a>(env: &mut JNIEnv<'a>, waker: Waker) -> Result<JObject<'a>> {
    let runnable = crate::ops::fn_once_runnable(env, |_e, _o| waker.wake())?;

    let obj = env.new_object(
        &crate::classcache::get_class("io/github/gedgygedgy/rust/task/Waker").unwrap(),
        "(Lio/github/gedgygedgy/rust/ops/FnRunnable;)V",
        &[(&runnable).into()],
    )?;
    Ok(obj)
}

/// Wrapper for [`JObject`]s that implement
/// `io.github.gedgygedgy.rust.task.PollResult`. Provides method to get the
/// poll result.
///
/// Looks up the class and method IDs on creation rather than for every method
/// call.
pub struct JPollResult<'a> {
    internal: JObject<'a>,
    get: JMethodID,
    env: JNIEnv<'a>,
}

impl<'a> JPollResult<'a> {
    /// Create a [`JPollResult`] from the environment and an object. This looks
    /// up the necessary class and method IDs to call all of the methods on it
    /// so that extra work doesn't need to be done on every method call.
    ///
    /// # Arguments
    ///
    /// * `env` - Java environment to use.
    /// * `obj` - Object to wrap.
    pub fn from_env(env: &mut JNIEnv<'a>, obj: JObject<'a>) -> Result<Self> {
        let get = env.get_method_id(
            &crate::classcache::get_class("io/github/gedgygedgy/rust/task/PollResult").unwrap(),
            "get",
            "()Ljava/lang/Object;",
        )?;
        Ok(Self {
            internal: obj,
            get,
            env: unsafe { env.unsafe_clone() },
        })
    }

    /// Gets the object associated with the [`JPollResult`] by calling
    /// `io.github.gedgygedgy.rust.task.PollResult.get()`. Can throw an
    /// exception.
    pub fn get(&self) -> Result<JObject<'a>> {
        let mut env = unsafe { self.env.unsafe_clone() };
        unsafe {
            env.call_method_unchecked(
                &self.internal,
                self.get,
                ReturnType::Object,
                &[],
            )
        }?
        .l()
    }
}

impl<'a> ::std::ops::Deref for JPollResult<'a> {
    type Target = JObject<'a>;

    fn deref(&self) -> &Self::Target {
        &self.internal
    }
}

impl<'a> From<JPollResult<'a>> for JObject<'a> {
    fn from(other: JPollResult<'a>) -> JObject<'a> {
        other.internal
    }
}

#[cfg(test)]
mod test {
    use crate::test_utils;
    use std::sync::Arc;

    #[test]
    fn test_waker_wake() {
        test_utils::JVM_ENV.with(|env| {
            let data = Arc::new(test_utils::TestWakerData::new());
            assert_eq!(Arc::strong_count(&data), 1);
            assert_eq!(data.value(), false);

            let waker = crate::test_utils::test_waker(&data);
            assert_eq!(Arc::strong_count(&data), 2);
            assert_eq!(data.value(), false);

            let jwaker = super::waker(env, waker).unwrap();
            assert_eq!(Arc::strong_count(&data), 2);
            assert_eq!(data.value(), false);

            env.call_method(jwaker, "wake", "()V", &[]).unwrap();
            assert_eq!(Arc::strong_count(&data), 1);
            assert_eq!(data.value(), true);
            data.set_value(false);

            env.call_method(jwaker, "wake", "()V", &[]).unwrap();
            assert_eq!(Arc::strong_count(&data), 1);
            assert_eq!(data.value(), false);
        });
    }

    #[test]
    fn test_waker_close_wake() {
        test_utils::JVM_ENV.with(|env| {
            let data = Arc::new(test_utils::TestWakerData::new());
            assert_eq!(Arc::strong_count(&data), 1);
            assert_eq!(data.value(), false);

            let waker = crate::test_utils::test_waker(&data);
            assert_eq!(Arc::strong_count(&data), 2);
            assert_eq!(data.value(), false);

            let jwaker = super::waker(env, waker).unwrap();
            assert_eq!(Arc::strong_count(&data), 2);
            assert_eq!(data.value(), false);

            env.call_method(jwaker, "close", "()V", &[]).unwrap();
            assert_eq!(Arc::strong_count(&data), 1);
            assert_eq!(data.value(), false);

            env.call_method(jwaker, "wake", "()V", &[]).unwrap();
            assert_eq!(Arc::strong_count(&data), 1);
            assert_eq!(data.value(), false);
        });
    }
}
