import { describe, it, expect, beforeAll, afterAll, beforeEach } from 'vitest';
import { initPool, closePool, createSession, writePacket, getPacket, getConnectionString } from '../index.js';
import { getPool } from '../pool.js';

const TEST_DB_URL = process.env.TEST_DATABASE_URL || getConnectionString();

describe('Packet Writing', () => {
  let testSessionId;

  beforeAll(async () => {
    initPool(TEST_DB_URL, { max: 5 });
    // Wait a bit for connection
    await new Promise(resolve => setTimeout(resolve, 100));
  });

  beforeEach(async () => {
    // Create a fresh session for each test
    testSessionId = await createSession();
  });

  afterAll(async () => {
    await closePool();
  });

  it('should write a packet with all metadata fields', async () => {
    const packetData = {
      sessionId: testSessionId,
      sessionTimeMs: 12345,
      packetNumber: 1n,
      serverVersion: '1.20.0',
      direction: 'clientbound',
      packet: { test: 'data' }
    };

    // Fire-and-forget write
    await writePacket(packetData);

    // Wait a bit for the async write to complete
    await new Promise(resolve => setTimeout(resolve, 100));

    // Verify by querying directly
    const pool = getPool();
    const result = await pool.query(
      'SELECT * FROM packets WHERE session_id = $1 AND packet_number = $2',
      [testSessionId, 1n]
    );

    expect(result.rows.length).toBe(1);
    const packet = result.rows[0];
    
    expect(packet.session_id).toBe(testSessionId);
    expect(Number(packet.session_time_ms)).toBe(12345);
    expect(Number(packet.packet_number)).toBe(1);
    expect(packet.server_version).toBe('1.20.0');
    expect(packet.direction).toBe('clientbound');
    expect(packet.packet).toEqual({ test: 'data' });
    expect(packet.ts).toBeDefined();
  });

  it('should write a packet with serverbound direction', async () => {
    const packetData = {
      sessionId: testSessionId,
      sessionTimeMs: 67890,
      packetNumber: 2n,
      serverVersion: '1.19.4',
      direction: 'serverbound',
      packet: { action: 'chat', message: 'hello' }
    };

    await writePacket(packetData);
    await new Promise(resolve => setTimeout(resolve, 100));

    const pool = getPool();
    const result = await pool.query(
      'SELECT * FROM packets WHERE session_id = $1 AND packet_number = $2',
      [testSessionId, 2n]
    );

    expect(result.rows.length).toBe(1);
    const packet = result.rows[0];
    expect(packet.direction).toBe('serverbound');
    expect(packet.server_version).toBe('1.19.4');
    expect(Number(packet.session_time_ms)).toBe(67890);
  });

  it('should write a packet with custom timestamp', async () => {
    const customTimestamp = new Date('2024-01-01T12:00:00Z');
    const packetData = {
      sessionId: testSessionId,
      sessionTimeMs: 1000,
      packetNumber: 3n,
      serverVersion: '1.20.1',
      direction: 'clientbound',
      packet: { type: 'custom' },
      ts: customTimestamp
    };

    await writePacket(packetData);
    await new Promise(resolve => setTimeout(resolve, 100));

    const pool = getPool();
    const result = await pool.query(
      'SELECT * FROM packets WHERE session_id = $1 AND packet_number = $2',
      [testSessionId, 3n]
    );

    expect(result.rows.length).toBe(1);
    const packet = result.rows[0];
    expect(new Date(packet.ts).getTime()).toBe(customTimestamp.getTime());
  });

  it('should write multiple packets for the same session', async () => {
    const packets = [
      {
        sessionId: testSessionId,
        sessionTimeMs: 100,
        packetNumber: 10n,
        serverVersion: '1.20.0',
        direction: 'clientbound',
        packet: { seq: 1 }
      },
      {
        sessionId: testSessionId,
        sessionTimeMs: 200,
        packetNumber: 11n,
        serverVersion: '1.20.0',
        direction: 'serverbound',
        packet: { seq: 2 }
      },
      {
        sessionId: testSessionId,
        sessionTimeMs: 300,
        packetNumber: 12n,
        serverVersion: '1.20.0',
        direction: 'clientbound',
        packet: { seq: 3 }
      }
    ];

    // Write all packets
    for (const packet of packets) {
      await writePacket(packet);
    }

    await new Promise(resolve => setTimeout(resolve, 200));

    const pool = getPool();
    const result = await pool.query(
      'SELECT * FROM packets WHERE session_id = $1 ORDER BY packet_number',
      [testSessionId]
    );

    expect(result.rows.length).toBe(3);
    expect(Number(result.rows[0].packet_number)).toBe(10);
    expect(Number(result.rows[1].packet_number)).toBe(11);
    expect(Number(result.rows[2].packet_number)).toBe(12);
  });

  it('should write packets with large packet numbers (bigint)', async () => {
    const largePacketNumber = BigInt('9223372036854775807'); // Max bigint
    const packetData = {
      sessionId: testSessionId,
      sessionTimeMs: 5000,
      packetNumber: largePacketNumber,
      serverVersion: '1.20.0',
      direction: 'clientbound',
      packet: { large: true }
    };

    await writePacket(packetData);
    await new Promise(resolve => setTimeout(resolve, 100));

    const pool = getPool();
    const result = await pool.query(
      'SELECT * FROM packets WHERE session_id = $1',
      [testSessionId]
    );

    expect(result.rows.length).toBe(1);
    expect(result.rows[0].packet_number).toBe('9223372036854775807');
  });

  it('should write packets with complex JSON packet data', async () => {
    const complexPacket = {
      nested: {
        data: {
          items: [1, 2, 3],
          metadata: {
            version: 1,
            flags: [true, false, true]
          }
        }
      },
      array: ['a', 'b', 'c'],
      number: 42,
      boolean: true,
      nullValue: null
    };

    const packetData = {
      sessionId: testSessionId,
      sessionTimeMs: 9999,
      packetNumber: 100n,
      serverVersion: '1.20.0',
      direction: 'clientbound',
      packet: complexPacket
    };

    await writePacket(packetData);
    await new Promise(resolve => setTimeout(resolve, 100));

    const pool = getPool();
    const result = await pool.query(
      'SELECT * FROM packets WHERE session_id = $1 AND packet_number = $2',
      [testSessionId, 100n]
    );

    expect(result.rows.length).toBe(1);
    expect(result.rows[0].packet).toEqual(complexPacket);
  });

  it('should handle fire-and-forget writes without blocking', async () => {
    const startTime = Date.now();
    
    // Write multiple packets without awaiting
    for (let i = 0; i < 10; i++) {
      writePacket({
        sessionId: testSessionId,
        sessionTimeMs: i * 100,
        packetNumber: BigInt(i),
        serverVersion: '1.20.0',
        direction: 'clientbound',
        packet: { index: i }
      });
    }
    
    const endTime = Date.now();
    const duration = endTime - startTime;
    
    // Fire-and-forget should return quickly (not wait for DB)
    expect(duration).toBeLessThan(100); // Should be very fast
    
    // Wait for writes to complete
    await new Promise(resolve => setTimeout(resolve, 500));
    
    // Verify packets were written
    const pool = getPool();
    const result = await pool.query(
      'SELECT COUNT(*) as count FROM packets WHERE session_id = $1',
      [testSessionId]
    );
    
    expect(Number(result.rows[0].count)).toBe(10);
  });

  it('should use default timestamp when not provided', async () => {
    const beforeWrite = new Date();
    
    const packetData = {
      sessionId: testSessionId,
      sessionTimeMs: 1234,
      packetNumber: 50n,
      serverVersion: '1.20.0',
      direction: 'clientbound',
      packet: { test: 'default timestamp' }
    };

    await writePacket(packetData);
    await new Promise(resolve => setTimeout(resolve, 100));

    const afterWrite = new Date();

    const pool = getPool();
    const result = await pool.query(
      'SELECT * FROM packets WHERE session_id = $1 AND packet_number = $2',
      [testSessionId, 50n]
    );

    expect(result.rows.length).toBe(1);
    const packetTimestamp = new Date(result.rows[0].ts);
    
    // Should be between before and after (with some tolerance)
    expect(packetTimestamp.getTime()).toBeGreaterThanOrEqual(beforeWrite.getTime() - 1000);
    expect(packetTimestamp.getTime()).toBeLessThanOrEqual(afterWrite.getTime() + 1000);
  });
});
