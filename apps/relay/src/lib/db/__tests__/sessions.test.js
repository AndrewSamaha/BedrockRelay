import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { initPool, closePool, createSession, endSession, getSession, getConnectionString } from '../index.js';

const TEST_DB_URL = process.env.TEST_DATABASE_URL || getConnectionString();

describe('Session Management', () => {
  beforeAll(async () => {
    initPool(TEST_DB_URL, { max: 5 });
    // Wait a bit for connection
    await new Promise(resolve => setTimeout(resolve, 100));
  });

  afterAll(async () => {
    await closePool();
  });

  it('should create a session with auto-increment ID', async () => {
    const sessionId = await createSession();
    
    expect(sessionId).toBeTypeOf('number');
    expect(sessionId).toBeGreaterThan(0);
  });

  it('should create a session with custom started_at timestamp', async () => {
    const customDate = new Date('2024-01-01T12:00:00Z');
    const sessionId = await createSession(customDate);
    
    expect(sessionId).toBeTypeOf('number');
    
    const session = await getSession(sessionId);
    expect(session).not.toBeNull();
    expect(session.id).toBe(sessionId);
    expect(new Date(session.started_at).getTime()).toBe(customDate.getTime());
  });

  it('should retrieve a session by ID', async () => {
    const sessionId = await createSession();
    const session = await getSession(sessionId);
    
    expect(session).not.toBeNull();
    expect(session.id).toBe(sessionId);
    expect(session.started_at).toBeDefined();
    expect(session.ended_at).toBeNull();
  });

  it('should return null for non-existent session', async () => {
    const session = await getSession(999999);
    expect(session).toBeNull();
  });

  it('should end a session by setting ended_at', async () => {
    const sessionId = await createSession();
    const endDate = new Date('2024-01-01T13:00:00Z');
    
    await endSession(sessionId, endDate);
    
    const session = await getSession(sessionId);
    expect(session.ended_at).not.toBeNull();
    expect(new Date(session.ended_at).getTime()).toBe(endDate.getTime());
  });

  it('should end a session with current timestamp if not provided', async () => {
    const sessionId = await createSession();
    const beforeEnd = new Date();
    
    await endSession(sessionId);
    
    const session = await getSession(sessionId);
    expect(session.ended_at).not.toBeNull();
    
    const afterEnd = new Date();
    const endedAt = new Date(session.ended_at);
    
    // Should be between before and after (with some tolerance)
    expect(endedAt.getTime()).toBeGreaterThanOrEqual(beforeEnd.getTime() - 1000);
    expect(endedAt.getTime()).toBeLessThanOrEqual(afterEnd.getTime() + 1000);
  });

  it('should create multiple sessions with sequential IDs', async () => {
    const id1 = await createSession();
    const id2 = await createSession();
    const id3 = await createSession();
    
    expect(id2).toBeGreaterThan(id1);
    expect(id3).toBeGreaterThan(id2);
  });
});
