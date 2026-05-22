import ws from 'k6/ws';
import { check, sleep } from 'k6';
import { Counter, Rate, Trend } from 'k6/metrics';

const vus = Number(__ENV.K6_WS_VUS || 100);
const duration = __ENV.K6_DURATION || '2m';

export const options = {
  scenarios: {
    chat_sockets: {
      executor: 'constant-vus',
      vus,
      duration,
    },
  },
  thresholds: {
    chat_ws_connect_latency: ['p(95)<1000'],
    chat_ws_session_duration: ['p(95)<70000'],
    ws_connection_failures: ['rate<0.01'],
    ws_messages_received: ['count>0'],
  },
};

const BASE_URL = (__ENV.BASE_URL || 'http://127.0.0.1:8080').replace(/\/$/, '');
const WS_BASE_URL = BASE_URL.replace(/^http/, 'ws');
const ROOM_ID = __ENV.ROOM_ID;
const ACCESS_TOKEN = __ENV.ACCESS_TOKEN;
const SESSION_MS = Number(__ENV.WS_SESSION_MS || 30000);
const SEND_INTERVAL_MS = Number(__ENV.WS_SEND_INTERVAL_MS || 10000);

const wsConnecting = new Trend('chat_ws_connect_latency');
const wsSessionDuration = new Trend('chat_ws_session_duration');
const wsConnectionFailures = new Rate('ws_connection_failures');
const wsMessagesReceived = new Counter('ws_messages_received');

if (!ROOM_ID || !ACCESS_TOKEN) {
  throw new Error('ROOM_ID and ACCESS_TOKEN are required for authenticated chat WebSocket benchmarks.');
}

export default function () {
  const url = `${WS_BASE_URL}/v1/chat/rooms/${ROOM_ID}/ws?access_token=${encodeURIComponent(
    ACCESS_TOKEN
  )}`;
  const started = Date.now();

  const res = ws.connect(url, {}, (socket) => {
    let openedAt = 0;

    socket.on('open', () => {
      openedAt = Date.now();
      wsConnecting.add(openedAt - started);
      socket.send(`k6 benchmark hello vu=${__VU} iter=${__ITER} at=${new Date().toISOString()}`);

      socket.setInterval(() => {
        socket.send(`k6 benchmark heartbeat vu=${__VU} iter=${__ITER} at=${new Date().toISOString()}`);
      }, SEND_INTERVAL_MS);
    });

    socket.on('message', () => {
      wsMessagesReceived.add(1);
    });

    socket.on('error', () => {
      wsConnectionFailures.add(true);
    });

    socket.setTimeout(() => {
      if (openedAt > 0) {
        wsSessionDuration.add(Date.now() - openedAt);
      }
      socket.close();
    }, SESSION_MS);
  });

  const ok = check(res, {
    'websocket upgraded': (r) => r && r.status === 101,
  });

  wsConnectionFailures.add(!ok);
  sleep(1);
}
