process.env.DEBUG = 'minecraft-protocol'

import dotenv from 'dotenv';
import { fileURLToPath } from 'url';
import { dirname, resolve } from 'path';

// Load .env from project root (two levels up from apps/relay/)
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const rootEnvPath = resolve(__dirname, '../../.env');
dotenv.config({ path: rootEnvPath });

import bedrockProtocol from 'bedrock-protocol';
const { Relay } = bedrockProtocol;
import { initPool, setupGracefulShutdown, registerShutdownHandler, createSession, endSession, writePacket, getConnectionString } from './src/lib/db/index.js';

// Initialize database connection
initPool(getConnectionString());

// Track active player sessions for graceful shutdown
const activePlayers = new Map(); // Map<sessionId, { player, sessionId }>

// Start your server first on port 19131.

// Start the proxy server
const relay = new Relay({
  version: process.env.BEDROCK_VERSION,
  host: process.env.PROXY_LISTENING_ADDRESS,
  port: Number(process.env.PROXY_LISTENING_PORT),
  destination: {
    host: process.env.PROXY_DESTINATION_ADDRESS,
    port: Number(process.env.PROXY_DESTINATION_PORT)
  }
})
relay.conLog = console.debug
relay.listen() // Tell the server to start listening.

// Register graceful shutdown handler
registerShutdownHandler(async () => {
  console.log('Shutting down relay server...');
  
  // Stop accepting new connections by removing the listener
  // This prevents new clients from connecting during shutdown
  relay.removeAllListeners('connect');
  
  // Disconnect all active players manually (avoiding relay.close() which has issues)
  console.log(`Disconnecting ${activePlayers.size} active client(s)...`);
  for (const { player } of activePlayers.values()) {
    try {
      // Disconnect the player directly
      if (player && typeof player.disconnect === 'function') {
        player.disconnect('Server shutting down');
      }
    } catch (error) {
      // Ignore errors during disconnect - we're shutting down anyway
    }
  }
  
  // Give a moment for disconnects to process
  await new Promise(resolve => setTimeout(resolve, 200));
  
  // End all active sessions in the database
  console.log(`Ending ${activePlayers.size} active session(s)...`);
  const endSessionPromises = Array.from(activePlayers.values()).map(async ({ sessionId }) => {
    try {
      await endSession(sessionId);
    } catch (error) {
      console.error(`Error ending session ${sessionId}:`, error);
    }
  });
  
  await Promise.all(endSessionPromises);
  console.log('All sessions ended');
  
  // Give a brief moment for any in-flight database writes to complete
  await new Promise(resolve => setTimeout(resolve, 500));
});

// Setup graceful shutdown (this will call our registered handler)
setupGracefulShutdown();

relay.on('connect', async (player) => {
  console.log('New connection', player.connection.address)

  // Create a new session for this connection
  const sessionId = await createSession();
  const sessionStartTime = Date.now();
  let packetNumber = 0n;

  // Store session info on the player object
  player.sessionId = sessionId;
  player.sessionStartTime = sessionStartTime;
  player.packetNumber = packetNumber;

  // Track this active session
  activePlayers.set(sessionId, { player, sessionId });

  // Set up periodic logging (every minute)
  const statsInterval = setInterval(() => {
    const sessionTimeMs = Date.now() - player.sessionStartTime;
    const sessionTimeSeconds = Math.floor(sessionTimeMs / 1000);
    const sessionTimeMinutes = Math.floor(sessionTimeSeconds / 60);
    const remainingSeconds = sessionTimeSeconds % 60;
    
    console.log(`Session ${sessionId}: ${Number(player.packetNumber)} packets, ${sessionTimeMinutes}m ${remainingSeconds}s`)
  }, 60000); // Every minute

  // Store interval so we can clear it on disconnect
  player.statsInterval = statsInterval;

  // Server is sending a message to the client.
  player.on('clientbound', (packet) => {
    const { name, params } = packet;
    
    // Increment packet number for this session
    player.packetNumber++;
    
    // Save packet to database (fire-and-forget)
    writePacket({
      sessionId: player.sessionId,
      sessionTimeMs: Date.now() - player.sessionStartTime,
      packetNumber: player.packetNumber,
      serverVersion: relay.options.version || process.env.BEDROCK_VERSION || 'unknown',
      direction: 'clientbound',
      packet: { name, params }
    });

    if (name === 'disconnect') { // Intercept kick
      params.message = 'Intercepted' // Change kick message to "Intercepted"
    }
  })
  
  // Client is sending a message to the server
  player.on('serverbound', (packet) => {
    const { name, params } = packet;
    
    // Increment packet number for this session
    player.packetNumber++;
    
    // Save packet to database (fire-and-forget)
    writePacket({
      sessionId: player.sessionId,
      sessionTimeMs: Date.now() - player.sessionStartTime,
      packetNumber: player.packetNumber,
      serverVersion: relay.options.version || process.env.BEDROCK_VERSION || 'unknown',
      direction: 'serverbound',
      packet: { name, params }
    });

    if (name === 'text') { // Intercept chat message to server and append time.
      params.message += `, on ${new Date().toLocaleString()}`
    }
  })

  // End session when player disconnects
  player.on('close', async () => {
    // Clear the stats interval
    if (player.statsInterval) {
      clearInterval(player.statsInterval);
    }

    // Remove from active players tracking
    activePlayers.delete(player.sessionId);

    // Log final stats
    const finalSessionTimeMs = Date.now() - player.sessionStartTime;
    const finalSessionTimeSeconds = Math.floor(finalSessionTimeMs / 1000);
    const finalSessionTimeMinutes = Math.floor(finalSessionTimeSeconds / 60);
    const finalRemainingSeconds = finalSessionTimeSeconds % 60;
    
    console.log(`Connection closed ${player.connection.address} - Session ${sessionId}: ${Number(player.packetNumber)} packets total, ${finalSessionTimeMinutes}m ${finalRemainingSeconds}s`)

    try {
      await endSession(player.sessionId);
    } catch (error) {
      console.error('Error ending session:', error);
    }
  })
})

// Now clients can connect to your proxy
