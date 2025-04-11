#![deny(clippy::all)]
#![allow(non_snake_case)]

#[macro_use]
extern crate napi_derive;

use napi::{
  CallContext, Env, JsFunction, JsNumber, JsObject, JsString, JsUnknown, Result, ValueType,
};
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
    Ok(Statement {
      stmt: Arc::new(Mutex::new(stmt)),
      conn: self.conn.clone(),
    })
  }

  #[napi]
  pub fn transaction(&self, env: Env, func: napi::JsFunction) -> Result<napi::JsFunction> {
    let conn = self.conn.clone();

    // Create a simple transaction function with empty mode
    let tx_function = transaction(env, &func, conn, "")?;

    // For now, just return the basic transaction function
    Ok(tx_function)
  }

  #[napi]
  pub fn pragma(&self) -> Result<()> {
    // TODO: Implement pragma
    Ok(())
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
  func: &napi::JsFunction,
  conn: Arc<Mutex<libsql::Connection>>,
  mode: &str,
) -> Result<napi::JsFunction> {
  let begin = format!("BEGIN {}", mode);
  let func_ref = env.create_reference(func)?;
  let tx_function = env.create_function_from_closure("transaction", move |ctx: CallContext| {
    let rt = runtime()?;
    let conn_guard = conn.lock().unwrap();
    rt.block_on(conn_guard.execute_batch(&begin))
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

#[napi]
pub struct Statement {
  stmt: Arc<Mutex<libsql::Statement>>,
  conn: Arc<Mutex<libsql::Connection>>,
}

#[napi(object)]
pub struct RunResult {
  pub changes: i64,
  pub duration: f64,
  pub lastInsertRowid: i64,
}

fn convert_params(
  env: &Env,
  stmt: &Arc<Mutex<libsql::Statement>>,
  params: Option<napi::JsUnknown>,
) -> Result<libsql::params::Params> {
  if let Some(params) = params {
    // Check if it's an array by trying to cast it
    if let Ok(object) = params.coerce_to_object() {
      if object.is_array()? {
        convert_params_array(env, object)
      } else {
        convert_params_object(env, stmt, object)
      }
    } else {
      // If we can't coerce to object, return empty params
      Ok(libsql::params::Params::None)
    }
  } else {
    Ok(libsql::params::Params::None)
  }
}

fn convert_params_array(env: &Env, object: napi::JsObject) -> Result<libsql::params::Params> {
  let mut params = vec![];
  
  // Get array length using the proper method
  let length = object.get_array_length()?;
  
  // Get array elements
  for i in 0..length {
    let element = object.get_element::<napi::JsUnknown>(i)?;
    let value = js_value_to_value(env, element)?;
    params.push(value);
  }
  
  Ok(libsql::params::Params::Positional(params))
}

fn convert_params_object(
  env: &Env,
  stmt: &Arc<Mutex<libsql::Statement>>,
  object: napi::JsObject,
) -> Result<libsql::params::Params> {
  let mut params = vec![];
  let stmt_guard = stmt.lock().unwrap();

  for idx in 0..stmt_guard.parameter_count() {
    let name = stmt_guard.parameter_name((idx + 1) as i32).unwrap();
    let name = name.to_string();

    // Remove the leading ':' or '@' or '$' from parameter name
    let key = &name[1..];

    if let Ok(value) = object.get_named_property::<napi::JsUnknown>(key) {
      let value = js_value_to_value(env, value)?;
      params.push((name, value));
    }
  }

  Ok(libsql::params::Params::Named(params))
}

fn js_value_to_value(env: &Env, value: napi::JsUnknown) -> Result<libsql::Value> {
  let value_type = value.get_type()?;

  match value_type {
    ValueType::Null | ValueType::Undefined => Ok(libsql::Value::Null),

    ValueType::Boolean => {
      let value = value.coerce_to_bool()?.get_value()?;
      Ok(libsql::Value::Integer(if value { 1 } else { 0 }))
    }

    ValueType::Number => {
      let value = value.coerce_to_number()?.get_double()?;
      Ok(libsql::Value::Real(value))
    }

    ValueType::String => {
      let js_string = value.coerce_to_string()?;
      let utf8 = js_string.into_utf8()?;
      Ok(libsql::Value::Text(utf8.as_str().unwrap().to_string()))
    }

    // Handle other types like Buffer for blobs
    _ => Err(napi::Error::from_reason(format!(
      "Unsupported parameter type: {:?}",
      value_type
    ))),
  }
}

#[napi]
impl Statement {
  #[napi]
  pub fn run(&self, params: Option<napi::JsUnknown>) -> Result<RunResult> {
    let rt = runtime()?;
    let conn_guard = self.conn.lock().unwrap();
    let total_changes_before = conn_guard.total_changes();

    // Get start time
    let start = std::time::Instant::now();

    // Get current environment - we pass null for env since we don't need it
    // Parameters will be converted in the Statement implementation
    let params = if let Some(params) = params {
      // Create a dummy env - we don't actually use it for anything critical
      let dummy_env = unsafe { Env::from_raw(std::ptr::null_mut()) };
      convert_params(&dummy_env, &self.stmt, Some(params))?
    } else {
      libsql::params::Params::None
    };

    // Execute the statement
    let mut stmt_guard = self.stmt.lock().unwrap();

    // Reset statement before execution
    stmt_guard.reset();

    // Execute with parameters
    let _ = rt.block_on(stmt_guard.query(params)).map_err(Error::from)?;

    // Calculate duration
    let duration = start.elapsed().as_secs_f64();

    // Get changes and last insert rowid
    let changes = if conn_guard.total_changes() == total_changes_before {
      0
    } else {
      conn_guard.changes() as i64
    };
    let last_insert_rowid = conn_guard.last_insert_rowid();

    Ok(RunResult {
      changes,
      duration,
      lastInsertRowid: last_insert_rowid,
    })
  }
}

fn runtime() -> Result<&'static Runtime> {
  static RUNTIME: OnceCell<Runtime> = OnceCell::new();

  let rt = RUNTIME.get_or_try_init(Runtime::new).unwrap();
  Ok(rt)
}
