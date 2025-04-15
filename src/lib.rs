#![deny(clippy::all)]
#![allow(non_snake_case)]

#[macro_use]
extern crate napi_derive;

use std::{cell::RefCell, sync::Arc};

use napi::{CallContext, Env, JsFunction, JsUnknown, Result, ValueType};
use once_cell::sync::OnceCell;
use tokio::{runtime::Runtime, sync::Mutex};

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
  conn: Option<Arc<tokio::sync::Mutex<libsql::Connection>>>,
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
    let conn_ = conn.clone();
    let stmt = rt
      .block_on(async move {
        let conn = conn_.lock().await;
        conn.prepare(&sql).await
      })
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
    rt.block_on(async move {
      let conn = conn.lock().await;
      conn.execute_batch(&sql).await
    })
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
  _conn: Arc<tokio::sync::Mutex<libsql::Connection>>,
  mode: &str,
) -> Result<napi::JsFunction> {
  let _begin = format!("BEGIN {}", mode);
  let _func_ref = env.create_reference(func)?;
  let tx_function = env.create_function_from_closure(
    "transaction",
    move |ctx: CallContext| -> Result<napi::JsFunction> {
      todo!();
    },
  )?;
  Ok(tx_function)
}

fn is_remote_path(path: &str) -> bool {
  path.starts_with("libsql://") || path.starts_with("http://") || path.starts_with("https://")
}

fn throw_database_closed_error(env: &Env) -> napi::Error {
  let msg = "The database connection is not open";
  let err = napi::Error::new(napi::Status::InvalidArg, msg.to_string());
  env.throw_type_error(&msg, None).unwrap();
  err
}

#[napi]
pub struct Statement {
  stmt: Arc<tokio::sync::Mutex<libsql::Statement>>,
  conn: Arc<tokio::sync::Mutex<libsql::Connection>>,
  safe_ints: RefCell<bool>,
  raw: RefCell<bool>,
}

#[napi(object)]
pub struct RunResult {
  pub changes: f64,
  pub duration: f64,
  pub lastInsertRowid: i64,
}

fn convert_params(
  stmt: &libsql::Statement,
  params: Option<napi::JsUnknown>,
) -> Result<libsql::params::Params> {
  if let Some(params) = params {
    match params.get_type()? {
      ValueType::Object => {
        let object = params.coerce_to_object()?;
        if object.is_array()? {
          convert_params_array(object)
        } else {
          convert_params_object(stmt, object)
        }
      }
      _ => convert_params_single(params),
    }
  } else {
    Ok(libsql::params::Params::None)
  }
}


fn convert_params_single(param: napi::JsUnknown) -> Result<libsql::params::Params> {
  Ok(libsql::params::Params::Positional(vec![js_value_to_value(param)?]))
}


fn convert_params_array(object: napi::JsObject) -> Result<libsql::params::Params> {
  let mut params = vec![];

  // Get array length using the proper method
  let length = object.get_array_length()?;

  // Get array elements
  for i in 0..length {
    let element = object.get_element::<napi::JsUnknown>(i)?;
    let value = js_value_to_value(element)?;
    params.push(value);
  }

  Ok(libsql::params::Params::Positional(params))
}

fn convert_params_object(
  stmt: &libsql::Statement,
  object: napi::JsObject,
) -> Result<libsql::params::Params> {
  let mut params = vec![];

  for idx in 0..stmt.parameter_count() {
    let name = stmt.parameter_name((idx + 1) as i32).unwrap();
    let name = name.to_string();

    // Remove the leading ':' or '@' or '$' from parameter name
    let key = &name[1..];

    if let Ok(value) = object.get_named_property::<napi::JsUnknown>(key) {
      let value = js_value_to_value(value)?;
      params.push((name, value));
    }
  }

  Ok(libsql::params::Params::Named(params))
}

fn js_value_to_value(value: napi::JsUnknown) -> Result<libsql::Value> {
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
    pub fn iterate(&self, env: Env, params: Option<napi::JsUnknown>) -> Result<napi::JsObject> {
        let rt = runtime()?;
        // Get safe_ints and raw flags
        let safe_ints = *self.safe_ints.borrow();
        let raw = *self.raw.borrow();
        let stmt = self.stmt.clone();
        // Lock statement and run query synchronously
        let rows = rt.block_on(async {
            let mut stmt = stmt.lock().await;
            stmt.reset();
            let params = if let Some(params) = params {
                convert_params(&stmt, Some(params)).unwrap()
            } else {
                libsql::params::Params::None
            };
            stmt.query(params).await.map_err(Error::from)
        })?;
        // Wrap rows in an iterator struct
        StatementRows::new(env, rows, safe_ints, raw)
    }

    #[napi]
    pub fn run(&self, params: Option<napi::JsUnknown>) -> Result<RunResult> {
      let rt = runtime()?;
      rt.block_on(async move {
        let conn = self.conn.lock().await;
        let total_changes_before = conn.total_changes();
        // Get start time
        let start = std::time::Instant::now();
  
        let mut stmt = self.stmt.lock().await;
        stmt.reset();
        let params = if let Some(params) = params {
          convert_params(&stmt, Some(params))?
        } else {
          libsql::params::Params::None
        };
        stmt.query(params).await.map_err(Error::from)?;
        let changes = if conn.total_changes() == total_changes_before {
          0
        } else {
          conn.changes()
        };
        let last_insert_row_id = conn.last_insert_rowid();
        // Calculate duration
        let duration = start.elapsed().as_secs_f64();
  
        Ok(RunResult {
          changes: changes as f64,
          duration,
          lastInsertRowid: last_insert_row_id,
        })
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
  
      // Execute the statement
      rt.block_on(async move {
        let mut stmt = self.stmt.lock().await;
        stmt.reset();
        let params = if let Some(params) = params {
          convert_params(&stmt, Some(params))?
        } else {
          libsql::params::Params::None
        };
        let mut rows = stmt.query(params).await.map_err(Error::from)?;
        let row = rows.next().await.map_err(Error::from)?;
        // Calculate duration
        let duration = start.elapsed().as_secs_f64();
        let result = match row {
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
        };
        stmt.reset();
        result
      })
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


#[napi]
pub struct StatementRows {
    rows: std::cell::RefCell<libsql::Rows>,
    safe_ints: bool,
    raw: bool,
    env: Env,
}

#[napi]
impl StatementRows {
    pub fn new(env: Env, rows: libsql::Rows, safe_ints: bool, raw: bool) -> Result<napi::JsObject> {
        let mut js_obj = env.create_object()?;
        let wrapper = StatementRows {
            rows: std::cell::RefCell::new(rows),
            safe_ints,
            raw,
            env: env.clone(),
        };
        let mut_ref = env.wrap(&mut js_obj, wrapper)?;
        // Attach next() method
        let next_fn = env.create_function_from_closure("next", move |ctx: CallContext| {
            let this = ctx.this_unchecked::<napi::JsObject>();
            let wrapper: &mut StatementRows = ctx.env.unwrap(&this)?;
            let rt = runtime()?;
            rt.block_on(async move {
              let mut rows = wrapper.rows.borrow_mut();
              let next_row = rows.next().await.map_err(Error::from)?;
              let mut result_obj = ctx.env.create_object()?;
              match next_row {
                  Some(row) => {
                      let value = if wrapper.raw {
                          convert_row_raw(&ctx.env, wrapper.safe_ints, &rows, &row)?
                      } else {
                          let mut js_object = ctx.env.create_object()?;
                          convert_row(&ctx.env, wrapper.safe_ints, &mut js_object, &rows, &row)?;
                          js_object.into_unknown()
                      };
                      result_obj.set_named_property("value", value)?;
                      result_obj.set_named_property("done", ctx.env.get_boolean(false)?)?;
                  }
                  None => {
                      result_obj.set_named_property("done", ctx.env.get_boolean(true)?)?;
                  }
              }
              Ok(result_obj)  
            })
        })?;
        js_obj.set_named_property("next", next_fn)?;
        Ok(js_obj)
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
