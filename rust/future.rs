use crate::task::JPollResult;
use ::jni::{
    errors::{Error, Result},
    objects::{GlobalRef, JMethodID, JObject, JValue},
    signature::ReturnType,
    JNIEnv, JavaVM,
};
use static_assertions::assert_impl_all;
use std::{
    convert::TryFrom,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Wrapper for [`JObject`]s that implement
/// `io.github.gedgygedgy.rust.future.Future`. Implements
/// [`Future`](std::future::Future) to allow asynchronous Rust code to wait for
/// a result from Java code.
///
/// Looks up the class and method IDs on creation rather than for every method
/// call.
///
/// For a [`Send`] version of this, use [`JSendFuture`].
pub struct JFuture<'a> {
    internal: JObject<'a>,
    poll: JMethodID,
    env: JNIEnv<'a>,
}

impl<'a> JFuture<'a> {
    /// Create a [`JFuture`] from the environment and an object. This looks up
    /// the necessary class and method IDs to call all of the methods on it so
    /// that extra work doesn't need to be done on every method call.
    ///
    /// # Arguments
    ///
    /// * `env` - Java environment to use.
    /// * `obj` - Object to wrap.
    pub fn from_env(env: &mut JNIEnv<'a>, obj: JObject<'a>) -> Result<Self> {
        let poll = env.get_method_id(
            &crate::classcache::get_class("io/github/gedgygedgy/rust/future/Future").unwrap(),
            "poll",
            "(Lio/github/gedgygedgy/rust/task/Waker;)Lio/github/gedgygedgy/rust/task/PollResult;",
        )?;
        Ok(Self {
            internal: obj,
            poll,
            env: unsafe { env.unsafe_clone() },
        })
    }

    /// Get the `io.github.gedgygedgy.rust.task.PollResult` from this future.
    /// Returns `null` if the future is not ready yet.
    ///
    /// # Arguments
    ///
    /// * `waker` - Waker object to wake later on if the result is not ready.
    pub fn poll(&self, waker: JObject<'a>) -> Result<JPollResult<'a>> {
        let mut env = unsafe { self.env.unsafe_clone() };
        let args = [JValue::from(&waker).as_jni()];
        let result = unsafe {
            env.call_method_unchecked(&self.internal, self.poll, ReturnType::Object, &args)
        }?
        .l()?;
        JPollResult::from_env(&mut env, result)
    }

    /// Turn the [`JFuture`] into a [`Future`] that can be `await`ed on.
    pub fn into_future(self) -> JFutureIntoFuture<'a> {
        JFutureIntoFuture(self)
    }
}

impl<'a> ::std::ops::Deref for JFuture<'a> {
    type Target = JObject<'a>;

    fn deref(&self) -> &Self::Target {
        &self.internal
    }
}

impl<'a> From<JFuture<'a>> for JObject<'a> {
    fn from(other: JFuture<'a>) -> JObject<'a> {
        other.internal
    }
}

/// Result of calling [`JFuture::into_future`]. This object can be `await`ed
/// to get a [`JPollResult`].
pub struct JFutureIntoFuture<'a>(JFuture<'a>);

impl<'a> JFutureIntoFuture<'a> {
    // Switch the Result and Poll return value to make this easier to implement using ?.
    fn poll_internal(&self, context: &mut Context<'_>) -> Result<Poll<JPollResult<'a>>> {
        use crate::task::waker;
        let mut env = unsafe { self.0.env.unsafe_clone() };
        let result = self.0.poll(waker(&mut env, context.waker().clone())?)?;
        Ok(if self.0.env.is_same_object(&*result, JObject::null())? {
            Poll::Pending
        } else {
            Poll::Ready(result)
        })
    }
}

impl<'a> Future for JFutureIntoFuture<'a> {
    type Output = Result<JPollResult<'a>>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match self.poll_internal(context) {
            Ok(Poll::Ready(result)) => Poll::Ready(Ok(result)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(err) => Poll::Ready(Err(err)),
        }
    }
}

impl<'a> From<JFutureIntoFuture<'a>> for JFuture<'a> {
    fn from(fut: JFutureIntoFuture<'a>) -> Self {
        fut.0
    }
}

impl<'a> std::ops::Deref for JFutureIntoFuture<'a> {
    type Target = JFuture<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// [`Send`] version of [`JFuture`]. Instead of storing a [`JNIEnv`], it stores
/// a [`JavaVM`] and calls [`JavaVM::get_env`] when [`Future::poll`] is called.
pub struct JSendFuture {
    internal: GlobalRef,
    vm: JavaVM,
}

impl<'a> TryFrom<JFuture<'a>> for JSendFuture {
    type Error = Error;

    fn try_from(future: JFuture<'a>) -> Result<Self> {
        Ok(Self {
            internal: future.env.new_global_ref(&future.internal)?,
            vm: future.env.get_java_vm()?,
        })
    }
}

impl ::std::ops::Deref for JSendFuture {
    type Target = GlobalRef;

    fn deref(&self) -> &Self::Target {
        &self.internal
    }
}

impl JSendFuture {
    fn poll_internal(&self, context: &mut Context<'_>) -> Result<Poll<Result<GlobalRef>>> {
        let mut env = self.vm.get_env()?;
        env.with_local_frame(32, |env| {
            let local = env.new_local_ref(&self.internal)?;
            let jfuture = JFuture::from_env(env, local)?.into_future();
            jfuture
                .poll_internal(context)
                .map(|result| result.map(|result| env.new_global_ref(&*result)))
        })
    }
}

impl Future for JSendFuture {
    type Output = Result<GlobalRef>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match self.poll_internal(context) {
            Ok(result) => result,
            Err(err) => Poll::Ready(Err(err)),
        }
    }
}

assert_impl_all!(JSendFuture: Send);

#[cfg(test)]
mod test {
    use super::{JFuture, JSendFuture};
    use crate::{task::JPollResult, test_utils};
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    };

    #[test]
    fn test_jfuture() {
        use std::sync::Arc;

        test_utils::JVM_ENV.with(|env| {
            let data = Arc::new(test_utils::TestWakerData::new());
            assert_eq!(Arc::strong_count(&data), 1);
            assert_eq!(data.value(), false);

            let waker = test_utils::test_waker(&data);
            assert_eq!(Arc::strong_count(&data), 2);
            assert_eq!(data.value(), false);

            let future_obj = env
                .new_object("io/github/gedgygedgy/rust/future/SimpleFuture", "()V", &[])
                .unwrap();
            let mut future = JFuture::from_env(env, future_obj).unwrap().into_future();

            assert!(
                Future::poll(Pin::new(&mut future), &mut Context::from_waker(&waker)).is_pending()
            );
            assert_eq!(Arc::strong_count(&data), 3);
            assert_eq!(data.value(), false);

            assert!(
                Future::poll(Pin::new(&mut future), &mut Context::from_waker(&waker)).is_pending()
            );
            assert_eq!(Arc::strong_count(&data), 3);
            assert_eq!(data.value(), false);

            let obj = env.new_object("java/lang/Object", "()V", &[]).unwrap();
            env.call_method(future_obj, "wake", "(Ljava/lang/Object;)V", &[obj.into()])
                .unwrap();
            assert_eq!(Arc::strong_count(&data), 2);
            assert_eq!(data.value(), true);

            let poll = Future::poll(Pin::new(&mut future), &mut Context::from_waker(&waker));
            if let Poll::Ready(result) = poll {
                assert!(env
                    .is_same_object(result.unwrap().get().unwrap(), obj)
                    .unwrap());
            } else {
                panic!("Poll result should be ready");
            }
            assert_eq!(Arc::strong_count(&data), 2);
            assert_eq!(data.value(), true);

            let poll = Future::poll(Pin::new(&mut future), &mut Context::from_waker(&waker));
            if let Poll::Ready(result) = poll {
                assert!(env
                    .is_same_object(result.unwrap().get().unwrap(), obj)
                    .unwrap());
            } else {
                panic!("Poll result should be ready");
            }
            assert_eq!(Arc::strong_count(&data), 2);
            assert_eq!(data.value(), true);
        });
    }

    #[test]
    fn test_jfuture_await() {
        use futures::{executor::block_on, join};

        test_utils::JVM_ENV.with(|env| {
            let future_obj = env
                .new_object("io/github/gedgygedgy/rust/future/SimpleFuture", "()V", &[])
                .unwrap();
            let future = JFuture::from_env(env, future_obj).unwrap();
            let obj = env.new_object("java/lang/Object", "()V", &[]).unwrap();

            block_on(async {
                join!(
                    async {
                        env.call_method(future_obj, "wake", "(Ljava/lang/Object;)V", &[obj.into()])
                            .unwrap();
                    },
                    async {
                        assert!(env
                            .is_same_object(future.into_future().await.unwrap().get().unwrap(), obj)
                            .unwrap());
                    }
                );
            });
        });
    }

    #[test]
    fn test_jfuture_await_throw() {
        use futures::{executor::block_on, join};

        test_utils::JVM_ENV.with(|env| {
            let future_obj = env
                .new_object("io/github/gedgygedgy/rust/future/SimpleFuture", "()V", &[])
                .unwrap();
            let future = JFuture::from_env(env, future_obj).unwrap();
            let ex = env.new_object("java/lang/Exception", "()V", &[]).unwrap();

            block_on(async {
                join!(
                    async {
                        env.call_method(
                            future_obj,
                            "wakeWithThrowable",
                            "(Ljava/lang/Throwable;)V",
                            &[ex.into()],
                        )
                        .unwrap();
                    },
                    async {
                        future.into_future().await.unwrap().get().unwrap_err();
                        let future_ex = env.exception_occurred().unwrap();
                        env.exception_clear().unwrap();
                        let actual_ex = env
                            .call_method(future_ex, "getCause", "()Ljava/lang/Throwable;", &[])
                            .unwrap()
                            .l()
                            .unwrap();
                        assert!(env.is_same_object(actual_ex, ex).unwrap());
                    }
                );
            });
        });
    }

    #[test]
    fn test_jsendfuture_await() {
        use futures::{executor::block_on, join};
        use std::convert::TryInto;

        test_utils::JVM_ENV.with(|env| {
            let future_obj = env
                .new_object("io/github/gedgygedgy/rust/future/SimpleFuture", "()V", &[])
                .unwrap();
            let future = JFuture::from_env(env, future_obj).unwrap();
            let future: JSendFuture = future.try_into().unwrap();
            let obj = env.new_object("java/lang/Object", "()V", &[]).unwrap();

            block_on(async {
                join!(
                    async {
                        env.call_method(future_obj, "wake", "(Ljava/lang/Object;)V", &[obj.into()])
                            .unwrap();
                    },
                    async {
                        let global_ref = future.await.unwrap();
                        let jpoll = JPollResult::from_env(env, global_ref.as_obj()).unwrap();
                        assert!(env.is_same_object(jpoll.get().unwrap(), obj).unwrap());
                    }
                );
            });
        });
    }
}
