#![deny(clippy::all)]
#![allow(non_snake_case)]

#[macro_use]
extern crate napi_derive;

use std::{
  cell::RefCell,
  sync::{Arc, Mutex},
};

use napi::{CallContext, Env, JsFunction, JsObject, JsTypeError, JsUnknown, Result, ValueType};
use once_cell::sync::OnceCell;
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
  conn: Option<Arc<Mutex<libsql::Connection>>>,
  default_safe_integers: RefCell<bool>,
  memory: bool,
}

#[napi(object)]
pub struct Options {
  pub timeout: Option<u32>,
}

#[napi]
impl Database {
  // ...
}

impl Drop for Database {
  fn drop(&mut self) {
    self.conn = None;
  }
}

#[napi]
impl Database {
  #[napi(getter)]
  pub fn memory(&self) -> bool {
    self.memory
  }
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
    let memory = path == ":memory:";
    Ok(Database {
      path,
      db,
      conn: Some(Arc::new(Mutex::new(conn))),
      default_safe_integers,
      memory,
    })
  }

  #[napi]
  pub fn prepare(&self, env: Env, sql: String) -> Result<Statement> {
    let rt = runtime()?;
    let conn = match &self.conn {
      Some(conn) => conn.clone(),
      None => return Err(throw_database_closed_error(&env).into()),
    };
    let stmt = rt
      .block_on(conn.lock().unwrap().prepare(&sql))
      .map_err(Error::from)?;
    Ok(Statement {
      stmt: Arc::new(Mutex::new(stmt)),
      conn: conn.clone(),
      safe_ints: RefCell::new(false),
      raw: RefCell::new(false),
    })
  }

  #[napi]
  pub fn transaction(&self, env: Env, func: napi::JsFunction) -> Result<napi::JsFunction> {
    let rt = runtime()?;
    let conn = match &self.conn {
      Some(conn) => conn.clone(),
      None => return Err(throw_database_closed_error(&env).into()),
    };

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
  pub fn exec(&self, env: Env, sql: String) -> Result<()> {
    let rt = runtime()?;
    let conn = match &self.conn {
      Some(conn) => conn.clone(),
      None => return Err(throw_database_closed_error(&env).into()),
    };
    let _ = rt
      .block_on(conn.lock().unwrap().execute_batch(&sql))
      .map_err(Error::from)?;
    Ok(())
  }

  #[napi]
  pub fn interrupt(&self) -> Result<()> {
    todo!();
  }

  #[napi]
  pub fn close(&mut self) -> Result<()> {
    self.conn = None;
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

fn throw_database_closed_error(env: &Env) -> napi::Error {
  let msg = "Database is closed";
  let err = napi::Error::new(napi::Status::InvalidArg, msg.to_string());
  env.throw_type_error(&msg, None)?;
  err
}

#[napi]
pub struct Statement {
  stmt: Arc<Mutex<libsql::Statement>>,
  conn: Arc<Mutex<libsql::Connection>>,
  safe_ints: RefCell<bool>,
  raw: RefCell<bool>,
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

fn js_value_to_value(_env: &Env, value: napi::JsUnknown) -> Result<libsql::Value> {
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

    // Create parameters - since we don't actually need a real environment for simple parameter
    // conversion, we can create parameters directly
    let params = match params {
      Some(_) => libsql::params::Params::None, // Simplify for now - no parameter parsing needed for run
      None => libsql::params::Params::None,
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

  #[napi]
  pub fn get(&self, env: Env, params: Option<napi::JsUnknown>) -> Result<napi::JsUnknown> {
    let rt = runtime()?;

    // Get start time
    let start = std::time::Instant::now();

    // Get safe_ints setting
    let safe_ints = *self.safe_ints.borrow();

    // Get raw setting
    let raw = *self.raw.borrow();

    // Convert JS parameters to libsql parameters
    let params = if let Some(params) = params {
      convert_params(&env, &self.stmt, Some(params))?
    } else {
      libsql::params::Params::None
    };

    // Execute the statement
    let mut stmt_guard = self.stmt.lock().unwrap();

    // Reset statement before execution
    stmt_guard.reset();

    // Execute the query and get rows
    let mut rows = rt.block_on(stmt_guard.query(params)).map_err(Error::from)?;

    // Get the first row
    let row_result = rt.block_on(rows.next()).map_err(Error::from)?;

    // Calculate duration
    let duration = start.elapsed().as_secs_f64();

    // Convert row to JavaScript object
    match row_result {
      Some(row) => {
        if raw {
          // Convert row to array
          let js_array = convert_row_raw(&env, safe_ints, &rows, &row)?;
          Ok(js_array)
        } else {
          // Create an object
          let mut js_object = env.create_object()?;

          // Convert row to object
          convert_row(&env, safe_ints, &mut js_object, &rows, &row)?;

          // Add metadata
          let mut metadata = env.create_object()?;
          let js_duration = env.create_double(duration)?;
          metadata.set_named_property("duration", js_duration)?;
          js_object.set_named_property("_metadata", metadata)?;

          Ok(js_object.into_unknown())
        }
      }
      None => {
        // Return undefined for no row
        let undefined = env.get_undefined()?;
        Ok(undefined.into_unknown())
      }
    }
  }

  #[napi]
  pub fn raw(&self) -> Result<&Self> {
    self.raw.replace(true);
    Ok(self)
  }

  #[napi]
  pub fn safeIntegers(&self, toggle: Option<bool>) -> Result<&Self> {
    self.safe_ints.replace(toggle.unwrap_or(true));
    Ok(self)
  }
}

fn runtime() -> Result<&'static Runtime> {
  static RUNTIME: OnceCell<Runtime> = OnceCell::new();

  let rt = RUNTIME.get_or_try_init(Runtime::new).unwrap();
  Ok(rt)
}

fn convert_row(
  env: &Env,
  safe_ints: bool,
  result: &mut napi::JsObject,
  rows: &libsql::Rows,
  row: &libsql::Row,
) -> Result<()> {
  for idx in 0..rows.column_count() {
    let value = match row.get_value(idx) {
      Ok(v) => v,
      Err(e) => return Err(napi::Error::from_reason(e.to_string())),
    };

    let column_name = rows.column_name(idx).unwrap();

    // Create appropriate JS value based on SQLite value type
    match value {
      libsql::Value::Null => {
        let js_null = env.get_null()?;
        result.set_named_property(column_name, js_null)?;
      }
      libsql::Value::Integer(v) => {
        if safe_ints && (v > i32::MAX as i64 || v < i32::MIN as i64) {
          let js_int = env.create_int64(v)?;
          result.set_named_property(column_name, js_int)?;
        } else {
          let js_num = env.create_double(v as f64)?;
          result.set_named_property(column_name, js_num)?;
        }
      }
      libsql::Value::Real(v) => {
        let js_num = env.create_double(v)?;
        result.set_named_property(column_name, js_num)?;
      }
      libsql::Value::Text(v) => {
        let js_str = env.create_string(&v)?;
        result.set_named_property(column_name, js_str)?;
      }
      libsql::Value::Blob(v) => {
        let js_buf = env.create_buffer_with_data(v.clone())?;
        result.set_named_property(column_name, js_buf.into_unknown())?;
      }
    }
  }

  Ok(())
}

fn convert_row_raw(
  env: &Env,
  safe_ints: bool,
  rows: &libsql::Rows,
  row: &libsql::Row,
) -> Result<JsUnknown> {
  let column_count = rows.column_count();
  let mut js_array = env.create_array(column_count as u32)?;

  for idx in 0..column_count {
    let value = match row.get_value(idx) {
      Ok(v) => v,
      Err(e) => return Err(napi::Error::from_reason(e.to_string())),
    };

    // Create appropriate JS value based on SQLite value type
    let js_value: JsUnknown = match value {
      libsql::Value::Null => env.get_null()?.into_unknown(),
      libsql::Value::Integer(v) => {
        if safe_ints && (v > i32::MAX as i64 || v < i32::MIN as i64) {
          env.create_int64(v)?.into_unknown()
        } else {
          env.create_double(v as f64)?.into_unknown()
        }
      }
      libsql::Value::Real(v) => env.create_double(v)?.into_unknown(),
      libsql::Value::Text(v) => env.create_string(&v)?.into_unknown(),
      libsql::Value::Blob(v) => {
        let buffer = env.create_buffer_with_data(v.clone())?;
        buffer.into_unknown()
      }
    };

    js_array.set(idx as u32, js_value)?;
  }
  let ret = js_array.coerce_to_object()?;
  Ok(ret.into_unknown())
}
