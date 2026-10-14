use jni::{JNIEnv, objects::GlobalRef};
use once_cell::sync::OnceCell;
use dashmap::DashMap;
use ::jni::{errors::Result};

static CLASSCACHE: OnceCell<DashMap<String, GlobalRef>> = OnceCell::new();

pub fn find_add_class(env: &mut JNIEnv, classname: &str) -> Result<()> {
  let cache = CLASSCACHE.get_or_init(|| DashMap::new());
  let class = env.find_class(classname).unwrap();
  let global = env.new_global_ref(class).unwrap();
  cache.insert(classname.to_owned(), global);
  Ok(())
}

pub fn get_class(classname: &str) -> Option<GlobalRef> {
  let cache = CLASSCACHE.get_or_init(|| DashMap::new());
  cache.get(classname).and_then(|pair| Some(pair.value().clone()))
}
