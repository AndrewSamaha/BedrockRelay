process.env.DEBUG = 'minecraft-protocol'

import 'dotenv/config';

import bedrockProtocol from 'bedrock-protocol';
const { Relay } = bedrockProtocol;

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

relay.on('connect', player => {
  console.log('New connection', player.connection.address)

  // Server is sending a message to the client.
  player.on('clientbound', (packet) => {
    const { name, params } = packet;
    if (name === 'disconnect') { // Intercept kick
      params.message = 'Intercepted' // Change kick message to "Intercepted"
    }
    console.log({ clientbound: name, packet })
  })
  // Client is sending a message to the server
  player.on('serverbound', (packet) => {
    const { name, params } = packet;
    if (name === 'text') { // Intercept chat message to server and append time.
      params.message += `, on ${new Date().toLocaleString()}`
    }
    console.log({ serverbound: name, packet })
  })
})

// Now clients can connect to your proxy
