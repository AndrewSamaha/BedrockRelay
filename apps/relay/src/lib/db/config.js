/**
 * Build a PostgreSQL connection string from environment variables
 * @returns {string} PostgreSQL connection string
 */
export function getConnectionString() {
  const host = process.env.DB_HOST || 'localhost';
  const port = process.env.DB_PORT || '5432';
  const user = process.env.DB_USER || 'postgres';
  const password = process.env.DB_PASSWORD || 'postgres';
  const database = process.env.DB_NAME || 'postgres';

  return `postgresql://${user}:${password}@${host}:${port}/${database}`;
}
