# Database Write Performance Evaluation & Improvement Plan

## Current Database Write Pattern Analysis

### Current Implementation

**Write Strategy:**
- One INSERT per packet (no batching)
- Fire-and-forget pattern (non-blocking, no backpressure)
- Synchronous serialization (BigInt cleanup, JSON prep) in event loop
- Default connection pool (20 connections)
- No queuing/buffering mechanism
- No performance metrics or monitoring
- Errors logged but not tracked

**Code Flow:**
1. Packet received in event handler
2. `writePacket()` called immediately
3. Synchronous serialization (BigInt ? string, null byte removal, object traversal)
4. `pool.query()` called without await (fire-and-forget)
5. Promise queued in pool
6. No feedback on success/failure or latency

### Potential Performance Issues

1. **High INSERT Frequency**: At high packet rates, many individual INSERT statements
2. **Serialization Overhead**: Recursive object traversal happens synchronously in event loop
3. **No Visibility**: Can't tell if writes are falling behind or queue is backing up
4. **Memory Growth**: Unhandled promises accumulate if DB can't keep up
5. **Limited Error Tracking**: Errors logged but no metrics on failure rates

## Steps to Measure and Improve Performance

### Phase 1: Measurement & Baseline

#### 1. Add Performance Metrics Collection
- Track write latency (time from `writePacket()` call to DB completion)
- Track write throughput (packets/second successfully written)
- Track pending writes (outstanding promises in pool)
- Track error rates (failed writes per second)
- Track pool utilization (active/idle connections)

#### 2. Add Database-Level Metrics
- Monitor PostgreSQL query performance (pg_stat_statements)
- Track INSERT throughput in PostgreSQL
- Monitor connection pool usage in PostgreSQL
- Track database CPU/IO utilization
- Monitor table size growth rate

#### 3. Add Application-Level Metrics
- Track memory usage (heap size, pending promises)
- Track event loop lag (measure time between packet receive and write initiation)
- Track serialization time (BigInt cleanup + JSON prep)
- Log metrics periodically (every 10-30 seconds)

#### 4. Establish Baseline Metrics
- Run load test with known packet rate (e.g., 100, 500, 1000, 5000 packets/sec)
- Measure current performance at each level
- Identify bottlenecks (serialization, network, DB, pool)
- Document current limits (max sustainable packet rate)

### Phase 2: Optimization Strategies

#### 5. Implement Batching
- Buffer packets in memory (e.g., 50-100 packets or 100ms window)
- Use PostgreSQL multi-value INSERT (e.g., `VALUES (...), (...), (...)`)
- Measure improvement: reduction in INSERT count, lower DB roundtrips

#### 6. Optimize Serialization
- Move serialization to worker thread or async queue
- Pre-allocate/reuse objects where possible
- Measure: event loop lag reduction

#### 7. Connection Pool Tuning
- Test different pool sizes (10, 20, 50, 100)
- Adjust based on packet rate and DB capacity
- Measure: optimal pool size for throughput vs. resource usage

#### 8. Implement Bulk Insert (PostgreSQL COPY)
- Use `COPY` for large batches (1000+ packets)
- Measure: significant throughput improvement for bulk operations

#### 9. Add Backpressure Mechanism
- Track queue depth (pending writes)
- If queue exceeds threshold, log warning or drop lowest-priority packets
- Measure: prevents memory growth and indicates when DB can't keep up

#### 10. Database Optimization
- Add database indexes based on query patterns
- Consider table partitioning by time (if high volume)
- Analyze query plans for INSERT operations
- Measure: reduced INSERT overhead

#### 11. Async Queue Pattern
- Use dedicated async queue (e.g., p-queue) for writes
- Separate serialization from network I/O
- Measure: better resource utilization, smoother backpressure

#### 12. Connection Pool Monitoring
- Log pool stats (waiting clients, active connections)
- Alert when pool is saturated
- Measure: identify when pool size needs adjustment

### Phase 3: Advanced Optimizations

#### 13. Time-Based Batching with Flush Triggers
- Buffer packets for X milliseconds (e.g., 50-200ms)
- Flush on: time threshold, buffer size threshold, or explicit flush
- Measure: balance between latency and throughput

#### 14. Partitioned Inserts
- If using table partitioning, route inserts to correct partition
- Measure: reduced lock contention

#### 15. Prepared Statements
- Use prepared statements for repeated INSERTs
- Measure: reduced query parsing overhead

#### 16. Connection Pool Per Session
- Consider dedicated write connections for high-traffic sessions
- Measure: reduced contention for high-volume sessions

#### 17. Write-Ahead Logging (WAL) Optimization
- Tune PostgreSQL WAL settings for high INSERT throughput
- Measure: improved write performance

#### 18. Memory Management
- Monitor and limit in-memory buffer sizes
- Implement circuit breaker if memory exceeds threshold
- Measure: prevent OOM crashes

### Phase 4: Monitoring & Alerting

#### 19. Real-time Dashboard
- Display metrics: packets/sec, write latency, queue depth, error rate
- Update every 1-5 seconds
- Measure: real-time visibility into system health

#### 20. Alerting
- Alert when: queue depth > threshold, error rate spikes, latency exceeds threshold
- Measure: proactive issue detection

#### 21. Historical Analysis
- Store metrics over time (TSDB like Prometheus)
- Identify patterns and trends
- Measure: capacity planning and optimization validation

## Measurable Success Criteria

### Before Optimization
- **Baseline**: X packets/sec, Y ms latency, Z% error rate

### After Optimization
- **Target**: 2-10x throughput improvement
- **Target**: 50-90% latency reduction
- **Target**: <0.1% error rate under normal load
- **Target**: Memory usage stays bounded

## Recommended Priority Order

1. **Add metrics (Phase 1)** - Understand current performance
2. **Implement batching (Phase 2, #5)** - Likely biggest win
3. **Optimize serialization (Phase 2, #6)** - Reduce event loop blocking
4. **Tune pool size (Phase 2, #7)** - Optimize resource usage
5. **Add backpressure (Phase 2, #9)** - Prevent resource exhaustion

## Implementation Notes

### Current Code Pattern
```javascript
// In relay.js - packet handlers
writePacket({
  sessionId: player.sessionId,
  sessionTimeMs: Date.now() - player.sessionStartTime,
  packetNumber: player.packetNumber,
  serverVersion: relay.options.version,
  direction: 'clientbound',
  packet: { name, params }
});

// In packets.js - writePacket function
export async function writePacket({...}) {
  const pool = getPool();
  const serializedPacket = serializeBigInts(packet); // Synchronous
  pool.query(INSERT_STATEMENT, [...params]).catch(...); // Fire-and-forget
}
```

### Key Bottlenecks to Address
1. **Synchronous serialization** - Blocks event loop
2. **Individual INSERTs** - High overhead at scale
3. **No batching** - Many roundtrips to database
4. **No visibility** - Can't measure or optimize what we can't see

### Expected Improvements
- **Batching**: 10-50x reduction in INSERT statements
- **Async serialization**: Reduced event loop blocking
- **Pool tuning**: Better resource utilization
- **Monitoring**: Data-driven optimization decisions
