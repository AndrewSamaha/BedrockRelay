import pkg from 'pg';
const { Pool } = pkg;

let pool = null;

/**
 * Initialize the database connection pool
 * @param {string} connectionString - PostgreSQL connection string
 * @param {object} options - Additional pool options
 * @returns {Pool} The database pool instance
 */
export function initPool(connectionString, options = {}) {
  if (pool) {
    return pool;
  }

  pool = new Pool({
    connectionString,
    max: options.max || 20,
    idleTimeoutMillis: options.idleTimeoutMillis || 30000,
    connectionTimeoutMillis: options.connectionTimeoutMillis || 2000,
    ...options
  });

  pool.on('error', (err) => {
    console.error('Unexpected error on idle client', err);
  });

  return pool;
}

/**
 * Get the current database pool instance
 * @returns {Pool} The database pool instance
 * @throws {Error} If pool is not initialized
 */
export function getPool() {
  if (!pool) {
    throw new Error('Database pool not initialized. Call initPool() first.');
  }
  return pool;
}

/**
 * Close the database connection pool gracefully
 * @returns {Promise<void>}
 */
export async function closePool() {
  if (pool) {
    await pool.end();
    pool = null;
  }
}
