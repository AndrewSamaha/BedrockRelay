import dotenv from 'dotenv';

// Load .env file
dotenv.config();

export default {
  test: {
    globals: true,
    environment: 'node'
  }
};
