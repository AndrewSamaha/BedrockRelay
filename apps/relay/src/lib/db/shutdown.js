import { closePool } from './pool.js';

let shutdownHandlers = [];

/**
 * Register a shutdown handler
 * @param {Function} handler - Function to call on shutdown
 */
export function registerShutdownHandler(handler) {
  shutdownHandlers.push(handler);
}

/**
 * Setup graceful shutdown handlers
 */
export function setupGracefulShutdown() {
  const shutdown = async (signal) => {
    console.log(`Received ${signal}, starting graceful shutdown...`);
    
    // Run all registered shutdown handlers
    for (const handler of shutdownHandlers) {
      try {
        await handler();
      } catch (error) {
        console.error('Error in shutdown handler:', error);
      }
    }
    
    // Close database pool
    try {
      await closePool();
      console.log('Database pool closed');
    } catch (error) {
      console.error('Error closing database pool:', error);
    }
    
    console.log('Graceful shutdown complete');
    process.exit(0);
  };

  process.on('SIGTERM', () => shutdown('SIGTERM'));
  process.on('SIGINT', () => shutdown('SIGINT'));
  
  // Handle uncaught exceptions
  process.on('uncaughtException', async (error) => {
    console.error('Uncaught exception:', error);
    await shutdown('uncaughtException');
  });
  
  // Handle unhandled promise rejections
  process.on('unhandledRejection', async (reason, promise) => {
    console.error('Unhandled rejection at:', promise, 'reason:', reason);
    await shutdown('unhandledRejection');
  });
}
