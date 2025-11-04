import { getPool } from './pool.js';

/**
 * Create a new session
 * @param {Date} startedAt - When the session started (defaults to now)
 * @returns {Promise<number>} The session ID
 */
export async function createSession(startedAt = new Date()) {
  const pool = getPool();
  
  try {
    const result = await pool.query(
      'INSERT INTO sessions (started_at) VALUES ($1) RETURNING id',
      [startedAt]
    );
    return result.rows[0].id;
  } catch (error) {
    console.error('Error creating session:', error);
    throw error;
  }
}

/**
 * End a session by setting the ended_at timestamp
 * @param {number} sessionId - The session ID
 * @param {Date} endedAt - When the session ended (defaults to now)
 * @returns {Promise<void>}
 */
export async function endSession(sessionId, endedAt = new Date()) {
  const pool = getPool();
  
  try {
    await pool.query(
      'UPDATE sessions SET ended_at = $1 WHERE id = $2',
      [endedAt, sessionId]
    );
  } catch (error) {
    console.error(`Error ending session ${sessionId}:`, error);
    throw error;
  }
}

/**
 * Get a session by ID
 * @param {number} sessionId - The session ID
 * @returns {Promise<object|null>} The session object or null if not found
 */
export async function getSession(sessionId) {
  const pool = getPool();
  
  try {
    const result = await pool.query(
      'SELECT id, started_at, ended_at FROM sessions WHERE id = $1',
      [sessionId]
    );
    return result.rows[0] || null;
  } catch (error) {
    console.error(`Error getting session ${sessionId}:`, error);
    throw error;
  }
}
