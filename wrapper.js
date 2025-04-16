"use strict";

const { Database: NativeDb, SqliteError } = require("./index.js");

function convertError(err) {
  if (err.libsqlError) {
    return new SqliteError(err.message, err.code, err.rawCode);
  }
  return err;
}

/**
 * Database represents a connection that can prepare and execute SQL statements.
 */
class Database {
  /**
   * Creates a new database connection. If the database file pointed to by `path` does not exists, it will be created.
   *
   * @constructor
   * @param {string} path - Path to the database file.
   */
  constructor(path, opts) {
    throw new Error("not implemented");
  }

  sync() {
    throw new Error("not implemented");
  }

  syncUntil(replicationIndex) {
    throw new Error("not implemented");
  }

  /**
   * Prepares a SQL statement for execution.
   *
   * @param {string} sql - The SQL statement string to prepare.
   */
  prepare(sql) {
    throw new Error("not implemented");
  }

  /**
   * Returns a function that executes the given function in a transaction.
   *
   * @param {function} fn - The function to wrap in a transaction.
   */
  transaction(fn) {
    throw new Error("not implemented");
  }

  pragma(source, options) {
    throw new Error("not implemented");
  }

  backup(filename, options) {
    throw new Error("not implemented");
  }

  serialize(options) {
    throw new Error("not implemented");
  }

  function(name, options, fn) {
    throw new Error("not implemented");
  }

  aggregate(name, options) {
    throw new Error("not implemented");
  }

  table(name, factory) {
    throw new Error("not implemented");
  }

  loadExtension(...args) {
    throw new Error("not implemented");
  }

  maxWriteReplicationIndex() {
    throw new Error("not implemented");
  }

  /**
   * Executes a SQL statement.
   *
   * @param {string} sql - The SQL statement string to execute.
   */
  exec(sql) {
    throw new Error("not implemented");
  }

  /**
   * Interrupts the database connection.
   */
  interrupt() {
    throw new Error("not implemented");
  }

  /**
   * Closes the database connection.
   */
  close() {
    throw new Error("not implemented");
  }

  /**
   * Toggle 64-bit integer support.
   */
  defaultSafeIntegers(toggle) {
    throw new Error("not implemented");
  }

  unsafeMode(...args) {
    throw new Error("not implemented");
  }
}

/**
 * Statement represents a prepared SQL statement that can be executed.
 */
class Statement {
  constructor(stmt) {
    throw new Error("not implemented");
  }

  /**
   * Toggle raw mode.
   *
   * @param raw Enable or disable raw mode. If you don't pass the parameter, raw mode is enabled.
   */
  raw(raw) {
    throw new Error("not implemented");
  }

  /**
   * Toggle pluck mode.
   *
   * @param pluckMode Enable or disable pluck mode. If you don't pass the parameter, pluck mode is enabled.
   */
  pluck(pluckMode) {
    throw new Error("not implemented");
  }

  get reader() {
    throw new Error("not implemented");
  }

  /**
   * Executes the SQL statement and returns an info object.
   */
  run(...bindParameters) {
    throw new Error("not implemented");
  }

  /**
   * Executes the SQL statement and returns the first row.
   *
   * @param bindParameters - The bind parameters for executing the statement.
   */
  get(...bindParameters) {
    throw new Error("not implemented");
  }

  /**
   * Executes the SQL statement and returns an iterator to the resulting rows.
   *
   * @param bindParameters - The bind parameters for executing the statement.
   */
  iterate(...bindParameters) {
    throw new Error("not implemented");
  }

  /**
   * Executes the SQL statement and returns an array of the resulting rows.
   *
   * @param bindParameters - The bind parameters for executing the statement.
   */
  all(...bindParameters) {
    throw new Error("not implemented");
  }

  /**
   * Interrupts the statement.
   */
  interrupt() {
    throw new Error("not implemented");
  }

  /**
   * Returns the columns in the result set returned by this prepared statement.
   */
  columns() {
    return statementColumns.call(this.stmt);
  }

  /**
   * Toggle 64-bit integer support.
   */
  safeIntegers(toggle) {
    throw new Error("not implemented");
  }
}

module.exports = Database;
module.exports.SqliteError = SqliteError;
