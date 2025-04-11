#![deny(clippy::all)]
#![allow(non_snake_case)]

#[macro_use]
extern crate napi_derive;

use napi::{CallContext, Env, JsFunction, JsUnknown, Result};
use once_cell::sync::OnceCell;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

struct Error(libsql::Error);

impl From<Error> for napi::Error {
  fn from(error: Error) -> Self {
    napi::Error::from_reason(error.0.to_string())
  }
}

impl From<libsql::Error> for Error {
  fn from(error: libsql::Error) -> Self {
    Error(error)
  }
}

#[napi]
pub struct Database {
  path: String,
  db: libsql::Database,
  conn: Arc<Mutex<libsql::Connection>>,
  default_safe_integers: RefCell<bool>,
}

#[napi(object)]
pub struct Options {
  pub timeout: Option<u32>,
}

#[napi]
pub struct Statement {
  stmt: libsql::Statement,
}

#[napi]
impl Database {
  #[napi(constructor)]
  pub fn new(path: String, _opts: Option<Options>) -> Result<Self> {
    let rt = runtime()?;
    let remote = is_remote_path(&path);
    let db = if remote {
      todo!("Remote databases are not supported yet");
    } else {
      let builder = libsql::Builder::new_local(&path);
      rt.block_on(builder.build()).map_err(Error::from)?
    };
    let conn = db.connect().map_err(Error::from)?;
    let default_safe_integers = RefCell::new(false);
    Ok(Database {
      path,
      db,
      conn: Arc::new(Mutex::new(conn)),
      default_safe_integers,
    })
  }

  #[napi]
  pub fn prepare(&self, sql: String) -> Result<Statement> {
    let rt = runtime()?;
    let conn = self.conn.lock().unwrap();
    let stmt = rt.block_on(conn.prepare(&sql)).map_err(Error::from)?;
    Ok(Statement { stmt })
  }

  #[napi]
  pub fn transaction(&self, env: Env, func: napi::JsFunction) -> Result<napi::JsFunction> {
    transaction(env, func, self.conn.clone())
  }

  #[napi]
  pub fn pragma(&self) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn backup(&self) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn serialize(&self) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn function(&self) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn aggregate(&self) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn table(&self) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn loadExtension(&self, _path: String) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn maxWriteReplicationIndex(&self) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn exec(&self, sql: String) -> Result<()> {
    let rt = runtime()?;
    let conn = self.conn.lock().unwrap();
    let _ = rt.block_on(conn.execute_batch(&sql)).map_err(Error::from)?;
    Ok(())
  }

  #[napi]
  pub fn interrupt(&self) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn close(&self) -> Result<()> {
    Ok(())
  }

  #[napi]
  pub fn defaultSafeIntegers(&self, toggle: bool) -> Result<()> {
    self.default_safe_integers.replace(toggle);
    Ok(())
  }

  #[napi]
  pub fn unsafeMode(&self) -> Result<()> {
    todo!();
  }
}

fn transaction(
  env: Env,
  func: napi::JsFunction,
  conn: Arc<Mutex<libsql::Connection>>,
) -> Result<napi::JsFunction> {
  let func_ref = env.create_reference(func)?;
  let tx_function = env.create_function_from_closure("transaction", move |ctx: CallContext| {
    let rt = runtime()?;
    let conn_guard = conn.lock().unwrap();
    rt.block_on(conn_guard.execute_batch("BEGIN"))
      .map_err(Error::from)?;

    let orig_fn = ctx.env.get_reference_value::<JsFunction>(&func_ref)?;

    let mut args = Vec::with_capacity(ctx.length as usize);
    for i in 0..ctx.length {
      args.push(ctx.get::<JsUnknown>(i)?);
    }

    let result = orig_fn.call(Some(&ctx.this_unchecked()), &args);

    match result {
      Ok(value) => {
        rt.block_on(conn_guard.execute_batch("COMMIT"))
          .map_err(Error::from)?;
        Ok(value)
      }
      Err(err) => {
        let _ = rt.block_on(conn_guard.execute_batch("ROLLBACK"));
        Err(err)
      }
    }
  })?;
  Ok(tx_function)
}

fn is_remote_path(path: &str) -> bool {
  path.starts_with("libsql://") || path.starts_with("http://") || path.starts_with("https://")
}

fn runtime() -> Result<&'static Runtime> {
  static RUNTIME: OnceCell<Runtime> = OnceCell::new();

  let rt = RUNTIME.get_or_try_init(Runtime::new).unwrap();
  Ok(rt)
}
