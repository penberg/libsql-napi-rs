#![deny(clippy::all)]
#![allow(non_snake_case)]
#![allow(deprecated)]

#[macro_use]
extern crate napi_derive;

use napi::{Env, JsUnknown, Result, ValueType, JsObject};
use napi::bindgen_prelude::{Buffer, FunctionCallContext, Array, JsFunction, Function};
use once_cell::sync::OnceCell;
use std::{cell::RefCell, sync::Arc};
use tokio::{runtime::Runtime, sync::Mutex};

#[napi]
pub struct SqliteError {
    #[napi]
    pub message: String,
    #[napi]
    pub code: String,
    #[napi(js_name = rawCode)]
    pub raw_code: Option<String>,
}

#[napi]
impl SqliteError {
    #[napi(constructor)]
    pub fn new(message: String, code: String, raw_code: Option<String>) -> Self {
        SqliteError { message, code, raw_code }
    }
}

struct Error(libsql::Error);

impl From<Error> for napi::Error {
    fn from(error: Error) -> Self {
        use libsql::Error as E;
        match &error.0 {
            E::SqliteFailure(code, msg) => {
                // code is c_int, convert to string like "SQLITE_CONSTRAINT" or fallback to number
                let code_str = libsql::ffi::code_to_str(*code);
                throw_sqlite_error(msg.clone(), code_str.to_string(), Some(code.to_string()))
            }
            E::RemoteSqliteFailure(_, code, msg) => {
                // code is i32, msg is String
                let code_str = libsql::ffi::code_to_str(*code);
                throw_sqlite_error(msg.clone(), code_str.to_string(), Some(code.to_string()))
            }
            _ => {
                throw_sqlite_error(error.0.to_string(), "SQLITE_ERROR".to_string(), None)
            }
        }
    }
}

pub fn throw_sqlite_error(message: String, code: String, raw_code: Option<String>) -> napi::Error {
    // 'message' must be the raw SQLite error string (e.g. 'near "SYNTAX": syntax error')
    let err_json = serde_json::json!({
        "libsqlError": true,
        "message": message, // do NOT wrap or format this string!
        "code": code,
        "rawCode": raw_code
    });
    napi::Error::from_reason(err_json.to_string())
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
            safe_ints: RefCell::new(*self.default_safe_integers.borrow()),
            raw: RefCell::new(false),
            pluck: RefCell::new(false),
        })
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
    pub fn defaultSafeIntegers(&self, toggle: Option<bool>) -> Result<()> {
        self.default_safe_integers.replace(toggle.unwrap_or(true));
        Ok(())
    }

    #[napi]
    pub fn unsafeMode(&self) -> Result<()> {
        todo!();
    }
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
    pluck: RefCell<bool>,
}

#[napi(object)]
pub struct RunResult {
    pub changes: f64,
    pub duration: f64,
    pub lastInsertRowid: i64,
}

fn map_params(
    stmt: &libsql::Statement,
    params: Option<napi::JsUnknown>,
) -> Result<libsql::params::Params> {
    if let Some(params) = params {
        match params.get_type()? {
            ValueType::Object => {
                let object = params.coerce_to_object()?;
                if object.is_array()? {
                    map_params_array(object)
                } else {
                    map_params_object(stmt, object)
                }
            }
            _ => map_params_single(params),
        }
    } else {
        Ok(libsql::params::Params::None)
    }
}

fn map_params_single(param: napi::JsUnknown) -> Result<libsql::params::Params> {
    Ok(libsql::params::Params::Positional(vec![map_value(param)?]))
}

fn map_params_array(object: napi::JsObject) -> Result<libsql::params::Params> {
    let mut params = vec![];

    // Get array length using the proper method
    let length = object.get_array_length()?;

    // Get array elements
    for i in 0..length {
        let element = object.get_element::<napi::JsUnknown>(i)?;
        let value = map_value(element)?;
        params.push(value);
    }

    Ok(libsql::params::Params::Positional(params))
}

fn map_params_object(
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
            let value = map_value(value)?;
            params.push((name, value));
        }
    }

    Ok(libsql::params::Params::Named(params))
}

/// Maps a JavaScript value to libSQL value types.
fn map_value(value: JsUnknown) -> Result<libsql::Value> {
    let value_type = value.get_type()?;

    match value_type {
        ValueType::Null | ValueType::Undefined => Ok(libsql::Value::Null),

        ValueType::Boolean => {
            let js_bool = value.coerce_to_bool()?;
            let b = js_bool.get_value()?;
            Ok(libsql::Value::Integer(if b { 1 } else { 0 }))
        }

        ValueType::Number => {
            let js_num = value.coerce_to_number()?;
            let n = js_num.get_double()?;
            Ok(libsql::Value::Real(n))
        }

        ValueType::String => {
            let js_str = value.coerce_to_string()?;
            let utf8 = js_str.into_utf8()?;
            // into_utf8 returns a Utf8 object that derefs to str
            Ok(libsql::Value::Text(utf8.as_str()?.to_owned()))
        }

        ValueType::Object => {
            let obj = value.coerce_to_object()?;

            // Check if it's a buffer
            if obj.is_buffer()? {
                let buf = napi::JsBuffer::try_from(obj.into_unknown())?;
                let data = buf.into_value()?.to_vec();
                return Ok(libsql::Value::Blob(data));
            }

            if obj.is_typedarray()? {
                let js_typed = napi::JsTypedArray::try_from(obj.into_unknown())?;
                let typed_array_value = js_typed.into_value()?;

                let buffer_data = typed_array_value.arraybuffer.into_value()?;
                let start = typed_array_value.byte_offset;
                let end = start + typed_array_value.length;

                if end > buffer_data.len() {
                    return Err(napi::Error::from_reason("TypedArray length out of bounds"));
                }

                let slice = &buffer_data[start..end];
                return Ok(libsql::Value::Blob(slice.to_vec()));
            }
            Err(napi::Error::from_reason(
                "SQLite3 can only bind numbers, strings, bigints, buffers, and null",
            ))
        }

        _ => Err(napi::Error::from_reason(
            "SQLite3 can only bind numbers, strings, bigints, buffers, and null",
        )),
    }
}

#[napi]
impl Statement {
    #[napi]
    pub fn columns(&self, env: Env) -> Result<Array> {
        let rt = runtime()?;
        let stmt = rt.block_on(self.stmt.lock());
        let columns = stmt.columns();
        let mut js_array = env.create_array(columns.len() as u32)?;
        for (i, col) in columns.iter().enumerate() {
            let mut js_obj = env.create_object()?;
            js_obj.set_named_property("name", env.create_string(col.name())?)?;
            // origin_name -> column
            if let Some(origin_name) = col.origin_name() {
                js_obj.set_named_property("column", env.create_string(origin_name)?)?;
            } else {
                js_obj.set_named_property("column", env.get_null()?)?;
            }
            // table_name -> table
            if let Some(table_name) = col.table_name() {
                js_obj.set_named_property("table", env.create_string(table_name)?)?;
            } else {
                js_obj.set_named_property("table", env.get_null()?)?;
            }
            // database_name -> database
            if let Some(database_name) = col.database_name() {
                js_obj.set_named_property("database", env.create_string(database_name)?)?;
            } else {
                js_obj.set_named_property("database", env.get_null()?)?;
            }
            // decl_type -> type
            if let Some(decl_type) = col.decl_type() {
                js_obj.set_named_property("type", env.create_string(decl_type)?)?;
            } else {
                js_obj.set_named_property("type", env.get_null()?)?;
            }
            js_array.set(i as u32, js_obj)?;
        }
        Ok(js_array)
    }
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
                map_params(&stmt, Some(params)).unwrap()
            } else {
                libsql::params::Params::None
            };
            stmt.query(params).await.map_err(Error::from)
        })?;
        // Wrap rows in an iterator struct
        StatementRows::new(env, Arc::new(tokio::sync::Mutex::new(rows)), safe_ints, raw)
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
                map_params(&stmt, Some(params))?
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
    pub fn all(&self, env: Env, params: Option<napi::JsUnknown>) -> Result<Array> {
        let rt = runtime()?;
        let safe_ints = *self.safe_ints.borrow();
        let raw = *self.raw.borrow();

        let mut rows = rt.block_on(async {
            let mut stmt = self.stmt.lock().await;
            stmt.reset();
            let params = if let Some(params) = params {
                map_params(&stmt, Some(params))?
            } else {
                libsql::params::Params::None
            };
            stmt.query(params)
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })?;

        let mut js_array = env.create_array(0)?;
        let mut idx = 0u32;
        let pluck = *self.pluck.borrow();
        while let Some(row) = rt.block_on(rows.next()).map_err(Error::from)? {
            let js_value = if raw {
                // Convert row to array
                let js_array = convert_row_raw(&env, safe_ints, &rows, &row)?;
                js_array.into_unknown()
            } else {
                // Create an object
                let mut js_object = env.create_object()?;

                // Convert row to object
                convert_row(&env, safe_ints, &mut js_object, &rows, &row)?;

                js_object.into_unknown()
            };
            // Pluck support: if pluck is enabled, extract the first column from the result
            let final_value = if pluck {
                if raw {
                    // js_value is an array/object, get index 0
                    let arr = js_value.coerce_to_object()?;
                    arr.get_element::<napi::JsUnknown>(0)?
                } else {
                    // js_value is an object, get the first property
                    let obj = js_value.coerce_to_object()?;
                    let keys = obj.get_property_names()?;
                    if keys.get_array_length()? > 0 {
                        let key = keys.get_element::<napi::JsString>(0)?;
                        obj.get_property(key)?
                    } else {
                        env.get_undefined()?.into_unknown()
                    }
                }
            } else {
                js_value
            };
            js_array.set(idx, final_value)?;
            idx += 1;
        }
        Ok(js_array)
    }

    #[napi]
    pub fn pluck(&self, pluck: Option<bool>) -> Result<&Self> {
        self.pluck.replace(pluck.unwrap_or(true));
        Ok(self)
    }

    #[napi]
    pub fn raw(&self, raw: Option<bool>) -> Result<&Self> {
        let rt = runtime()?;
        let returns_data = rt.block_on(async move {
            let stmt = self.stmt.lock().await;
            !stmt.columns().is_empty()
        });
        if !returns_data {
            return Err(napi::Error::from_reason(
                "The raw() method is only for statements that return data",
            ));
        }
        self.raw.replace(raw.unwrap_or(true));
        Ok(self)
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
                map_params(&stmt, Some(params))?
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
                        Ok(js_array.into_unknown())
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
    pub fn safeIntegers(&self, toggle: Option<bool>) -> Result<&Self> {
        self.safe_ints.replace(toggle.unwrap_or(true));
        Ok(self)
    }
}

#[napi]
pub struct StatementRows {
    rows: Arc<tokio::sync::Mutex<libsql::Rows>>,
    safe_ints: bool,
    raw: bool,
    env: Env,
}

#[napi]
impl StatementRows {
    pub fn new(
        env: Env,
        rows: Arc<tokio::sync::Mutex<libsql::Rows>>,
        safe_ints: bool,
        raw: bool,
    ) -> Result<napi::JsObject> {
        let mut js_obj = env.create_object()?;
        let next_fn: napi::bindgen_prelude::Function<'_, (), napi::JsObject> = env.create_function_from_closure("next", move |ctx: FunctionCallContext| {
            let rt = runtime()?;
            let rows = rows.clone();
            rt.block_on(async move {
                let mut rows = rows.lock().await;
                let next_row = rows.next().await.map_err(Error::from)?;
                let mut result_obj = ctx.env.create_object()?;
                match next_row {
                    Some(row) => {
                        let value = if raw {
                            convert_row_raw(&ctx.env, safe_ints, &rows, &row)?.into_unknown()
                        } else {
                            let mut js_object = ctx.env.create_object()?;
                            convert_row(&ctx.env, safe_ints, &mut js_object, &rows, &row)?;
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
        // Create iterator function
let iterator_fn: Function<'_, (), Result<JsObject>> = env.create_function_from_closure("iterator", move |ctx: FunctionCallContext| {
    Ok(ctx.this())
})?;

        // Get Symbol.iterator
        let global = env.get_global()?;
        let symbol_ctor = global
            .get_named_property::<JsFunction>("Symbol")?;
        let symbol_ctor_obj = symbol_ctor.coerce_to_object()?;
        let symbol_iterator = symbol_ctor_obj.get_named_property::<napi::JsSymbol>("iterator")?;
        // Attach [Symbol.iterator]
        js_obj.set_property(symbol_iterator, iterator_fn)?;
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
                if safe_ints {
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
                let js_buf = Buffer::from(v.clone());
                result.set_named_property(column_name, js_buf)?;
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
        let js_value = match value {
            libsql::Value::Null => Ok(env.get_null()?.into_unknown()),
            libsql::Value::Integer(v) => {
                if safe_ints {
                    Ok(env.create_bigint_from_i64(v)?.into_unknown()?)
                } else {
                    Ok(env.create_double(v as f64)?.into_unknown())
                }
            },
            libsql::Value::Real(v) => Ok(env.create_double(v)?.into_unknown()),
            libsql::Value::Text(v) => Ok(env.create_string(&v)?.into_unknown()),
            libsql::Value::Blob(v) => env.create_buffer_with_data(v.clone()).map(|b| b.into_unknown()), 
        }?;

        js_array.set(idx as u32, js_value)?;
    }
    Ok(js_array.coerce_to_object()?.into_unknown())
}
