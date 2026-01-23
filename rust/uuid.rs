use jni::{
    errors::Result,
    objects::{AutoLocal, JClass, JMethodID, JObject},
    signature::{Primitive, ReturnType},
    sys::jlong,
    JNIEnv,
};
use uuid::Uuid;

/// Wrapper for [`JObject`]s that contain `java.util.UUID`. Provides methods
/// to convert to and from a [`Uuid`].
///
/// Looks up the class and method IDs on creation rather than for every method
/// call.
pub struct JUuid<'a> {
    internal: JObject<'a>,
    get_least_significant_bits: JMethodID,
    get_most_significant_bits: JMethodID,
    env: JNIEnv<'a>,
}

impl<'a> JUuid<'a> {
    /// Create a [`JUuid`] from the environment and an object. This looks up
    /// the necessary class and method IDs to call all of the methods on it so
    /// that extra work doesn't need to be done on every method call.
    ///
    /// # Arguments
    ///
    /// * `env` - Java environment to use.
    /// * `obj` - Object to wrap.
    pub fn from_env(env: &mut JNIEnv<'a>, obj: JObject<'a>) -> Result<Self> {
        let class = env.find_class("java/util/UUID")?;
        let class = env.auto_local(class);
        Self::from_env_impl(env, obj, class)
    }

    /// Create a [`JUuid`] which wraps a new `java.util.UUID` created from a
    /// given [`Uuid`].
    ///
    /// # Arguments
    ///
    /// * `env` - Java environment to use.
    /// * `uuid` - [`Uuid`] to convert into a `java.util.UUID`.
    pub fn new(env: &mut JNIEnv<'a>, uuid: Uuid) -> Result<Self> {
        let val = uuid.as_u128();
        let least = (val & 0xFFFFFFFFFFFFFFFF) as jlong;
        let most = ((val >> 64) & 0xFFFFFFFFFFFFFFFF) as jlong;

        let class = env.find_class("java/util/UUID")?;
        let class = env.auto_local(class);
        let obj = env.new_object(&class, "(JJ)V", &[most.into(), least.into()])?;
        Self::from_env_impl(env, obj, class)
    }

    /// Convert the `java.util.UUID` into a [`Uuid`].
    pub fn as_uuid(&self) -> Result<Uuid> {
        let mut env = unsafe { self.env.unsafe_clone() };
        let least = unsafe {
            env.call_method_unchecked(
                &self.internal,
                self.get_least_significant_bits,
                ReturnType::Primitive(Primitive::Long),
                &[],
            )
        }?
        .j()? as u64;
        let most = unsafe {
            env.call_method_unchecked(
                &self.internal,
                self.get_most_significant_bits,
                ReturnType::Primitive(Primitive::Long),
                &[],
            )
        }?
        .j()? as u64;
        let val = ((most as u128) << 64) | (least as u128);
        Ok(Uuid::from_u128(val))
    }

    fn from_env_impl(
        env: &mut JNIEnv<'a>,
        obj: JObject<'a>,
        class: AutoLocal<'a, JClass<'a>>,
    ) -> Result<Self> {
        let get_least_significant_bits =
            env.get_method_id(&class, "getLeastSignificantBits", "()J")?;
        let get_most_significant_bits =
            env.get_method_id(&class, "getMostSignificantBits", "()J")?;
        Ok(Self {
            internal: obj,
            get_least_significant_bits,
            get_most_significant_bits,
            env: unsafe { env.unsafe_clone() },
        })
    }
}

impl<'a> ::std::ops::Deref for JUuid<'a> {
    type Target = JObject<'a>;

    fn deref(&self) -> &Self::Target {
        &self.internal
    }
}

impl<'a> From<JUuid<'a>> for JObject<'a> {
    fn from(other: JUuid<'a>) -> JObject<'a> {
        other.internal
    }
}

#[cfg(test)]
mod test {
    use super::JUuid;
    use crate::test_utils;
    use jni::{objects::JObject, sys::jlong};
    use uuid::Uuid;

    struct UuidTest {
        uuid: u128,
        most: u64,
        least: u64,
    }

    const TESTS: &[UuidTest] = &[
        UuidTest {
            uuid: 0x63f0f617_f589_40d0_98be_90747b1ea55a,
            most: 0x63f0f617_f589_40d0,
            least: 0x98be_90747b1ea55a,
        },
        UuidTest {
            uuid: 0xdea61ec0_51a6_4d97_81e0_d7b77e9c03d4,
            most: 0xdea61ec0_51a6_4d97,
            least: 0x81e0_d7b77e9c03d4,
        },
    ];

    #[test]
    fn test_uuid_new() {
        test_utils::JVM_ENV.with(|env| {
            for test in TESTS {
                let most = test.most as jlong;
                let least = test.least as jlong;

                let uuid_obj = JUuid::new(env, Uuid::from_u128(test.uuid)).unwrap();
                let obj: JObject = uuid_obj.into();

                let actual_most = env
                    .call_method(obj, "getMostSignificantBits", "()J", &[])
                    .unwrap()
                    .j()
                    .unwrap();
                let actual_least = env
                    .call_method(obj, "getLeastSignificantBits", "()J", &[])
                    .unwrap()
                    .j()
                    .unwrap();
                assert_eq!(actual_most, most);
                assert_eq!(actual_least, least);
            }
        });
    }

    #[test]
    fn test_uuid_as_uuid() {
        test_utils::JVM_ENV.with(|env| {
            for test in TESTS {
                let most = test.most as jlong;
                let least = test.least as jlong;

                let obj = env
                    .new_object("java/util/UUID", "(JJ)V", &[most.into(), least.into()])
                    .unwrap();
                let uuid_obj = JUuid::from_env(env, obj).unwrap();

                assert_eq!(uuid_obj.as_uuid().unwrap(), Uuid::from_u128(test.uuid));
            }
        });
    }
}
