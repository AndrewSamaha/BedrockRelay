import { getPool } from './pool.js';

/**
 * Write a packet to the database (fire-and-forget)
 * This function does not block and errors are logged but not thrown
 * @param {object} packetData - The packet data
 * @param {number} packetData.sessionId - The session ID
 * @param {number} packetData.sessionTimeMs - Session time in milliseconds
 * @param {bigint|number} packetData.packetNumber - The packet number
 * @param {string} packetData.serverVersion - The server version (semver)
 * @param {string} packetData.direction - Packet direction ('clientbound' or 'serverbound')
 * @param {object} packetData.packet - The packet JSON object
 * @param {Date} packetData.ts - When the packet was captured (defaults to now)
 * @returns {Promise<void>}
 */
/**
 * Recursively convert BigInt values to strings for JSON serialization
 * and remove null bytes from strings (PostgreSQL JSONB doesn't support \u0000)
 * @param {any} obj - The object to process
 * @returns {any} - Object with BigInts converted to strings and null bytes removed
 */
function serializeBigInts(obj) {
  if (obj === null || obj === undefined) {
    return obj;
  }
  
  if (typeof obj === 'bigint') {
    return obj.toString();
  }
  
  if (typeof obj === 'string') {
    // Remove null bytes - PostgreSQL JSONB doesn't support \u0000
    return obj.replace(/\u0000/g, '');
  }
  
  if (Array.isArray(obj)) {
    return obj.map(item => serializeBigInts(item));
  }
  
  if (typeof obj === 'object' && obj.constructor === Object) {
    const result = {};
    for (const [key, value] of Object.entries(obj)) {
      result[key] = serializeBigInts(value);
    }
    return result;
  }
  
  return obj;
}

export async function writePacket({
  sessionId,
  sessionTimeMs,
  packetNumber,
  serverVersion,
  direction,
  packet,
  ts = new Date()
}) {
  const pool = getPool();
  
  // Serialize BigInts in the packet object for JSON
  const serializedPacket = serializeBigInts(packet);
  
  // Convert packetNumber BigInt to number for PostgreSQL bigint
  // PostgreSQL BIGINT can handle numbers up to 2^63-1, but JavaScript Number is safe up to 2^53-1
  // For very large packet numbers, convert to string
  const packetNumberValue = typeof packetNumber === 'bigint' 
    ? (packetNumber <= Number.MAX_SAFE_INTEGER ? Number(packetNumber) : packetNumber.toString())
    : Number(packetNumber);
  
  // Fire-and-forget: don't await, just log errors
  pool.query(
    `INSERT INTO packets (session_id, ts, session_time_ms, packet_number, server_version, direction, packet)
     VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)`,
    [sessionId, ts, sessionTimeMs, packetNumberValue, serverVersion, direction, serializedPacket]
  ).catch((error) => {
    console.error('Error writing packet (fire-and-forget):', error);
    // Don't rethrow - this is fire-and-forget
  });
}

/**
 * Get a packet by ID (for testing/debugging)
 * @param {number} packetId - The packet ID
 * @returns {Promise<object|null>} The packet object or null if not found
 */
export async function getPacket(packetId) {
  const pool = getPool();
  
  try {
    const result = await pool.query(
      'SELECT * FROM packets WHERE id = $1',
      [packetId]
    );
    return result.rows[0] || null;
  } catch (error) {
    console.error(`Error getting packet ${packetId}:`, error);
    throw error;
  }
}
