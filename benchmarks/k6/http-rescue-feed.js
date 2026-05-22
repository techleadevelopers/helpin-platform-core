import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const vus = Number(__ENV.K6_VUS || 50);
const duration = __ENV.K6_DURATION || '2m';

export const options = {
  scenarios: {
    rescue_read_path: {
      executor: 'constant-vus',
      vus,
      duration,
    },
  },
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(95)<700', 'p(99)<1500'],
    feed_latency: ['p(95)<700'],
    geo_latency: ['p(95)<900'],
    search_latency: ['p(95)<700'],
    ready_latency: ['p(95)<250'],
    api_contract_failures: ['rate<0.01'],
  },
};

const BASE_URL = (__ENV.BASE_URL || 'http://127.0.0.1:8080').replace(/\/$/, '');
const LAT = __ENV.LAT || '-23.5505';
const LNG = __ENV.LNG || '-46.6333';
const RADIUS_KM = __ENV.RADIUS_KM || '25';
const ACCESS_TOKEN = __ENV.ACCESS_TOKEN || '';

const feedLatency = new Trend('feed_latency');
const geoLatency = new Trend('geo_latency');
const searchLatency = new Trend('search_latency');
const readyLatency = new Trend('ready_latency');
const contractFailures = new Rate('api_contract_failures');

function headers() {
  return ACCESS_TOKEN
    ? { headers: { Authorization: `Bearer ${ACCESS_TOKEN}` } }
    : {};
}

function timedGet(metric, name, path, params = {}) {
  const started = Date.now();
  const res = http.get(`${BASE_URL}${path}`, params);
  metric.add(Date.now() - started);

  const ok = check(res, {
    [`${name} status is 2xx`]: (r) => r.status >= 200 && r.status < 300,
  });
  contractFailures.add(!ok);
  return res;
}

export default function () {
  timedGet(readyLatency, 'healthz', '/healthz');
  timedGet(readyLatency, 'readyz', '/readyz');

  timedGet(
    feedLatency,
    'feed nearby emergency',
    `/v1/feed?lat=${LAT}&lng=${LNG}&radius_km=${RADIUS_KM}&limit=30`
  );

  timedGet(
    geoLatency,
    'geo nearby',
    `/v1/geo/nearby?lat=${LAT}&lng=${LNG}&radius_km=${RADIUS_KM}&limit=30`
  );

  timedGet(searchLatency, 'search city/rescue', '/v1/search?q=Campinas&limit=20');

  if (ACCESS_TOKEN) {
    timedGet(readyLatency, 'notifications', '/v1/notifications', headers());
  }

  sleep(Number(__ENV.K6_SLEEP_SECONDS || 1));
}
