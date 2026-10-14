use futures::{task::noop_waker, Stream};
use jni::{objects::GlobalRef, InitArgsBuilder, JNIEnv, JavaVM};
use jni_utils::{
    future::{JFuture, JSendFuture},
    stream::{JSendStream, JStream},
};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

fn weak_reference(env: &mut JNIEnv, object: &GlobalRef) -> GlobalRef {
    let weak = env
        .new_object(
            "java/lang/ref/WeakReference",
            "(Ljava/lang/Object;)V",
            &[object.as_obj().into()],
        )
        .unwrap();
    env.new_global_ref(weak).unwrap()
}

fn assert_collected(env: &mut JNIEnv, weak: &GlobalRef) {
    for _ in 0..32 {
        env.call_static_method("java/lang/System", "gc", "()V", &[])
            .unwrap();
        let collected = env
            .with_local_frame(8, |env| -> jni::errors::Result<bool> {
                Ok(env
                    .call_method(weak.as_obj(), "get", "()Ljava/lang/Object;", &[])?
                    .l()?
                    .is_null())
            })
            .unwrap();
        if collected {
            return;
        }
    }
    panic!("completed JNI operation still retains its Java object");
}

#[test]
fn send_poll_does_not_retain_java_objects_on_attached_thread() {
    let mut jar = std::env::current_exe().unwrap();
    jar.pop();
    jar.pop();
    jar.push("java/libs/jni-utils-0.1.1-SNAPSHOT.jar");
    assert!(
        jar.exists(),
        "run with --features build-java-support,jni/invocation"
    );
    let classpath = format!("-Djava.class.path={}", jar.display());
    let args = InitArgsBuilder::new().option(&classpath).build().unwrap();
    let vm = JavaVM::new(args).unwrap();
    let mut env = vm.attach_current_thread_permanently().unwrap();
    jni_utils::init(&mut env).unwrap();
    let waker = noop_waker();
    let mut context = Context::from_waker(&waker);
    for iteration in 0..32 {
        let ready = iteration % 2 == 0;
        let (mut stream, weak_stream, weak_item) = env
            .with_local_frame(16, |env| -> jni::errors::Result<_> {
                let obj =
                    env.new_object("io/github/gedgygedgy/rust/stream/QueueStream", "()V", &[])?;
                let global = env.new_global_ref(&obj)?;
                let item = env.new_object("java/lang/Object", "()V", &[])?;
                let weak_item = weak_reference(env, &env.new_global_ref(&item)?);
                if ready {
                    env.call_method(&obj, "add", "(Ljava/lang/Object;)V", &[(&item).into()])?;
                }
                Ok((
                    JSendStream::try_from(JStream::from_env(env, obj)?)?,
                    weak_reference(env, &global),
                    weak_item,
                ))
            })
            .unwrap();
        let poll = Pin::new(&mut stream).poll_next(&mut context);
        assert_eq!(poll.is_ready(), ready);
        if let Poll::Ready(Some(Ok(ref item))) = poll {
            env.with_local_frame(8, |env| -> jni::errors::Result<()> {
                env.call_static_method("java/lang/System", "gc", "()V", &[])?;
                let expected = env
                    .call_method(weak_item.as_obj(), "get", "()Ljava/lang/Object;", &[])?
                    .l()?;
                assert!(!expected.is_null());
                assert!(env.is_same_object(item.as_obj(), expected)?);
                Ok(())
            })
            .unwrap();
        }
        drop(poll);
        drop(stream);
        assert_collected(&mut env, &weak_stream);
        assert_collected(&mut env, &weak_item);

        let (mut future, weak_future) = env
            .with_local_frame(16, |env| -> jni::errors::Result<_> {
                let obj =
                    env.new_object("io/github/gedgygedgy/rust/future/SimpleFuture", "()V", &[])?;
                let global = env.new_global_ref(&obj)?;
                if ready {
                    env.call_method(
                        &obj,
                        "wake",
                        "(Ljava/lang/Object;)V",
                        &[(&jni::objects::JObject::null()).into()],
                    )?;
                }
                Ok((
                    JSendFuture::try_from(JFuture::from_env(env, obj)?)?,
                    weak_reference(env, &global),
                ))
            })
            .unwrap();
        let poll = Pin::new(&mut future).poll(&mut context);
        assert_eq!(matches!(poll, Poll::Ready(Ok(_))), ready);
        let weak_result = if let Poll::Ready(Ok(ref result)) = poll {
            Some(
                env.with_local_frame(8, |env| -> jni::errors::Result<_> {
                    assert!(env
                        .call_method(result.as_obj(), "get", "()Ljava/lang/Object;", &[])?
                        .l()?
                        .is_null());
                    Ok(weak_reference(env, result))
                })
                .unwrap(),
            )
        } else {
            None
        };
        drop(poll);
        drop(future);
        assert_collected(&mut env, &weak_future);
        if let Some(weak_result) = weak_result {
            assert_collected(&mut env, &weak_result);
        }
    }

    let (mut stream, weak_stream) = env
        .with_local_frame(16, |env| -> jni::errors::Result<_> {
            let obj = env.new_object("io/github/gedgygedgy/rust/stream/QueueStream", "()V", &[])?;
            let global = env.new_global_ref(&obj)?;
            Ok((
                JSendStream::try_from(JStream::from_env(env, obj)?)?,
                weak_reference(env, &global),
            ))
        })
        .unwrap();
    env.throw_new("java/lang/IllegalStateException", "injected poll failure")
        .unwrap();
    assert!(matches!(
        Pin::new(&mut stream).poll_next(&mut context),
        Poll::Ready(Some(Err(_)))
    ));
    assert!(env.exception_check().unwrap());
    env.exception_clear().unwrap();
    drop(stream);
    assert_collected(&mut env, &weak_stream);
}
